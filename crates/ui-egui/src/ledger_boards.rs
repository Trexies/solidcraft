//! The Ledger tab: the shop's boards, put into the design.
//!
//! Ledger is the shop's own program, and it publishes `data/boards.json` beside its database: one entry per board the shop
//! stocks, with the sheet's size, its colour and its density (when someone has weighed it). This tab reads that file and
//! adds a body for the board chosen - in the shop's colour, and with its physical material when the shop has weighed it.
//!
//! Nothing here is invented. A board that is not in the file is not offered; one with no density becomes a colour and not a
//! material; and a file that cannot be read says so rather than showing an empty shop.

use crate::SolveApp;
use crate::theme::Tokens;
use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;
use std::path::PathBuf;
use std::time::SystemTime;

/// One board, as Ledger publishes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    pub material: String,
    pub colour: String,
    pub width: f64,
    pub height: f64,
    pub thickness: f64,
    pub density_g_cm3: Option<f64>,
}

impl Board {
    /// What the body is called in the design: "Asian Night sheet".
    fn body_label(&self) -> String {
        format!("{} sheet", self.material)
    }

    /// The board's size, as the shop says it: "2743 × 1829 × 18".
    fn size_label(&self) -> String {
        format!("{} × {} × {}", mm(self.width), mm(self.height), mm(self.thickness))
    }

    /// The board's weight in kilograms, when the shop has weighed the material.
    fn kilograms(&self) -> Option<f64> {
        let density = self.density_g_cm3?;
        // mm3 to cm3, grams to kilograms: two thousandths, so one division by a million.
        Some(self.width * self.height * self.thickness * density / 1_000_000.0)
    }

    /// The board's colour, or the shop's default when the file holds something that is not one.
    fn rgb(&self) -> Color32 {
        parse_hex(&self.colour).unwrap_or(Color32::from_rgb(0xb0, 0x70, 0x40))
    }
}

/// A number of millimetres without a trailing `.0`.
fn mm(value: f64) -> String {
    if (value.fract()).abs() < 0.05 { format!("{value:.0}") } else { format!("{value:.1}") }
}

/// `#rrggbb`, or nothing when the text is not that.
fn parse_hex(text: &str) -> Option<Color32> {
    let text = text.trim();
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?))
}

/// The design key the shop's own data lives under: a reverse-DNS object, which is what SolidCraft reserves for another
/// tool's data. The board tags live here, inside the design, so the rule survives saving, reopening and another session.
const KEY: &str = "shop.ledger";

/// How far a board may drift from its class and still be that board: a face drag that moves a tenth of a millimetre.
const SLACK_MM: f64 = 0.25;

/// Whether the guard is already running, so its own undo does not call it again.
static GUARDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The boards recorded in this design: body name to its tag.
fn design_boards(app: &mut SolveApp) -> serde_json::Map<String, serde_json::Value> {
    app.run("document.metadata", json!({}))
        .ok()
        .and_then(|answer| answer.get("properties")?.get(KEY)?.get("boards")?.as_object().cloned())
        .unwrap_or_default()
}

/// Record a board's limits in the design, under the name of the body that carries it.
fn record(app: &mut SolveApp, name: &str, board: &Board) -> Result<(), String> {
    let mut boards = design_boards(app);
    boards.insert(
        name.to_owned(),
        json!({
            "material": board.material,
            "width": board.width,
            "height": board.height,
            "thickness": board.thickness,
            "class": solidcraft_class(board),
            "sheet": sheet_name(board),
        }),
    );
    app.run("document.metadata.set", json!({ "values": { KEY: { "boards": boards } } }))?;
    Ok(())
}

/// The thickness class of a board, in the shop's own words, for the record.
fn solidcraft_class(board: &Board) -> String {
    for (mm, name) in [(12.7, "1/2\""), (19.05, "3/4\""), (25.4, "1\""), (28.575, "1-1/8\"")] {
        if (board.thickness - mm).abs() <= 0.25 {
            return name.to_owned();
        }
    }
    format!("{:.2} mm", board.thickness)
}

/// The sheet a board is cut from, as the shop says it, or the board's own size for another material.
fn sheet_name(board: &Board) -> String {
    for (short, long, name) in [(1219.0, 2438.0, "4x8"), (1525.0, 2743.0, "5x9")] {
        let fits = |a: f64, b: f64| (a - short).abs() <= 1.0 && (b - long).abs() <= 1.0;
        if fits(board.width, board.height) || fits(board.height, board.width) {
            return name.to_owned();
        }
    }
    format!("{:.0} x {:.0}", board.width, board.height)
}

/// Hold every board in the design to the size it was made at.
///
/// A board is stock, not a sketch: its thickness is one of four classes and it cannot become bigger than its sheet. SolidCraft
/// has no opinion about that, so this runs after a command that changed the design, measures the tagged bodies, and takes the
/// change back when one of them has been stretched. Cutting a board smaller is allowed - that is what a saw does.
pub fn enforce(app: &mut SolveApp) {
    use std::sync::atomic::Ordering;
    if GUARDING.swap(true, Ordering::SeqCst) {
        return;
    }
    let broken = check(app);
    if let Some(why) = broken {
        let _ = app.run("edit.undo", json!({}));
        app.set_status(why, true);
    }
    GUARDING.store(false, Ordering::SeqCst);
}

/// Why a board in the design is no longer stock, or nothing when they all are.
fn check(app: &mut SolveApp) -> Option<String> {
    let boards = design_boards(app);
    if boards.is_empty() {
        return None;
    }
    let names: Vec<String> = boards.keys().cloned().collect();
    let measured = app.run("inspect.measure", json!({ "bodies": names })).ok()?;
    for body in measured.get("bodies")?.as_array()? {
        let name = body.get("name")?.as_str()?;
        let Some(tag) = boards.get(name) else { continue };
        let bbox = body.get("bbox")?;
        let min = bbox.get("min")?.as_array()?;
        let max = bbox.get("max")?.as_array()?;
        let mut size: Vec<f64> = (0..3)
            .map(|axis| {
                max.get(axis).and_then(serde_json::Value::as_f64).unwrap_or(0.0) - min.get(axis).and_then(serde_json::Value::as_f64).unwrap_or(0.0)
            })
            .collect();
        size.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // The smallest dimension is the thickness however the board is turned; the other two are its face.
        let thickness = size[0];
        let wanted = tag.get("thickness").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        if wanted > 0.0 && (thickness - wanted).abs() > SLACK_MM {
            return Some(format!(
                "{name} is a {} board: its thickness cannot become {thickness:.1} mm",
                tag.get("class").and_then(serde_json::Value::as_str).unwrap_or("stock")
            ));
        }
        let (long, short) = (size[2], size[1]);
        let (few_long, few_short) = sorted_sides(tag);
        if long > few_long + SLACK_MM || short > few_short + SLACK_MM {
            return Some(format!(
                "{name} is cut from a {} sheet: {long:.0} x {short:.0} mm does not fit",
                tag.get("sheet").and_then(serde_json::Value::as_str).unwrap_or("stock")
            ));
        }
    }
    None
}

/// A tag's sheet sides, longest first, so they compare with a measured board's.
fn sorted_sides(tag: &serde_json::Value) -> (f64, f64) {
    let width = tag.get("width").and_then(serde_json::Value::as_f64).unwrap_or(f64::INFINITY);
    let height = tag.get("height").and_then(serde_json::Value::as_f64).unwrap_or(f64::INFINITY);
    if width >= height { (width, height) } else { (height, width) }
}

/// Where Ledger publishes the boards: `LEDGER_BOARDS`, else the shop's own folder.
pub fn file() -> PathBuf {
    if let Some(path) = std::env::var_os("LEDGER_BOARDS") {
        return PathBuf::from(path);
    }
    PathBuf::from(r"D:\Ledger\data\boards.json")
}

/// What was read last, and when the file said it changed.
#[derive(Default)]
struct Cache {
    stamp: Option<SystemTime>,
    boards: Vec<Board>,
    error: Option<String>,
}

fn cache() -> &'static std::sync::Mutex<Cache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(Cache::default()))
}

/// The boards, read again when the file's timestamp moves. `reload` forces it, for a shop that has just added one.
pub fn boards(reload: bool) -> (Vec<Board>, Option<String>) {
    let path = file();
    let stamp = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
    let mut cache = cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !reload && stamp.is_some() && stamp == cache.stamp {
        return (cache.boards.clone(), cache.error.clone());
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => match parse(&text) {
            Ok(boards) => {
                cache.stamp = stamp;
                cache.boards = boards.clone();
                cache.error = None;
                (boards, None)
            }
            Err(error) => {
                cache.stamp = stamp;
                cache.boards.clear();
                cache.error = Some(format!("{}: {error}", path.display()));
                (Vec::new(), cache.error.clone())
            }
        },
        Err(error) => {
            cache.stamp = None;
            cache.boards.clear();
            cache.error = Some(format!("{}: {error}", path.display()));
            (Vec::new(), cache.error.clone())
        }
    }
}

/// Read the published boards. A file without the shop's format marker is refused rather than guessed at.
pub fn parse(text: &str) -> Result<Vec<Board>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if value.get("format").and_then(serde_json::Value::as_str) != Some("ledger-boards/1") {
        return Err("not a Ledger boards file".to_owned());
    }
    let entries = value.get("boards").and_then(serde_json::Value::as_array).ok_or("no boards in the file")?;
    let number = |v: &serde_json::Value, key: &str| v.get(key).and_then(serde_json::Value::as_f64);
    let mut boards = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(material) = entry.get("material").and_then(serde_json::Value::as_str) else { continue };
        let (Some(width), Some(height), Some(thickness)) = (number(entry, "width"), number(entry, "height"), number(entry, "thickness")) else {
            continue;
        };
        if !(width > 0.0 && height > 0.0 && thickness > 0.0) {
            continue;
        }
        boards.push(Board {
            material: material.to_owned(),
            colour: entry.get("colour").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned(),
            width,
            height,
            thickness,
            density_g_cm3: number(entry, "density_g_cm3").filter(|density| *density > 0.0),
        });
    }
    Ok(boards)
}

/// Whether the design already has a body by this name.
fn taken(app: &SolveApp, name: &str) -> bool {
    app.session.world_state().bodies.iter().any(|body| body.name == name)
}

/// A body name the design does not use yet: "Asian Night sheet", then "Asian Night sheet (2)".
fn free_name(app: &SolveApp, base: &str) -> String {
    if !taken(app, base) {
        return base.to_owned();
    }
    for n in 2.. {
        let candidate = format!("{base} ({n})");
        if !taken(app, &candidate) {
            return candidate;
        }
    }
    unreachable!("the counter runs until a name is free")
}

/// Put one board into the design, and answer the body's name.
///
/// The order matters: the physical material exists first (SolidCraft assigns by name), then the body, then its look, then
/// the material itself, so a failure leaves a coloured board rather than a board with a material that is not there.
pub fn insert(app: &mut SolveApp, board: &Board) -> Result<String, String> {
    let colour = format!("#{:02x}{:02x}{:02x}", board.rgb().r(), board.rgb().g(), board.rgb().b());
    if let Some(density) = board.density_g_cm3 {
        let listed = app.run("material.list", json!({}))?;
        if !listed_has(&listed, &board.material) {
            app.run("material.create", json!({ "name": board.material, "density_g_cm3": density, "color": colour }))?;
        }
    }
    let name = free_name(app, &board.body_label());
    // Lay the board flat beyond whatever is already there, as sheets are laid out on a bench, rather than inside it.
    let x = bench_end(app)?;
    app.run(
        "solid.box",
        json!({
            "length": board.width,
            "width": board.height,
            "height": board.thickness,
            "center": [x + board.width / 2.0, board.height / 2.0, board.thickness / 2.0],
            "name": name,
            "body_name": name,
        }),
    )?;
    app.run("appearance.assign", json!({ "bodies": [name], "color": colour }))?;
    if board.density_g_cm3.is_some() {
        app.run("material.assign", json!({ "bodies": [name], "material": board.material }))?;
    }
    // The design keeps the board's own limits, so the guard still knows them after a save and a reopen.
    record(app, &name, board)?;
    app.fit_view();
    Ok(name)
}

/// Where the next board starts: past the right-hand edge of everything in the design, with a gap to walk between.
fn bench_end(app: &mut SolveApp) -> Result<f64, String> {
    const GAP_MM: f64 = 150.0;
    let measure = app.run("inspect.measure", json!({}))?;
    let right = measure
        .get("bodies")
        .and_then(serde_json::Value::as_array)
        .map(|bodies| bodies.iter().filter_map(|body| body.get("bbox")?.get("max")?.get(0)?.as_f64()).fold(0.0_f64, f64::max))
        .unwrap_or(0.0);
    Ok(if right > 0.0 { right + GAP_MM } else { 0.0 })
}

/// Whether `material.list`'s answer carries a material by this name.
fn listed_has(answer: &serde_json::Value, name: &str) -> bool {
    answer
        .get("materials")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|list| list.iter().any(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(name)))
}

/// The tab's panel: one card per board, with the shop's colour and the board's size.
pub fn ribbon(app: &mut SolveApp, ui: &mut egui::Ui, area: Rect) {
    let t = Tokens::get();
    let (shop_boards, error) = boards(false);
    let mut insert_at: Option<Board> = None;
    let mut reload = false;

    let pad = t.space_8;
    let card_width = 208.0;
    let card_height = (area.height() - 2.0 * pad).max(28.0);
    let mut x = area.left() + pad;
    let top = area.top() + pad;
    ui.set_clip_rect(area);

    if let Some(error) = &error {
        ui.painter().text(
            pos2(x, top + card_height / 2.0),
            Align2::LEFT_CENTER,
            format!("Ledger's boards could not be read - {error}"),
            crate::fonts::semibold(t.font_small),
            t.text_dim,
        );
        crate::scenario::publish_handle("ledger:boards_error", pos2(x, top + card_height / 2.0));
        return;
    }
    if shop_boards.is_empty() {
        ui.painter().text(
            pos2(x, top + card_height / 2.0),
            Align2::LEFT_CENTER,
            "Ledger has no boards yet - add a material there, with its sheet size",
            crate::fonts::semibold(t.font_small),
            t.text_dim,
        );
        return;
    }

    for board in &shop_boards {
        let card = Rect::from_min_size(pos2(x, top), vec2(card_width, card_height));
        if card.right() > area.right() - pad {
            break;
        }
        let response = ui.interact(card, ui.id().with(("ledger_board", board.material.as_str())), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{} board", board.material)));
        let hovered = response.hovered();
        let corner = t.control_radius;
        ui.painter().rect(
            card,
            corner,
            if hovered { t.hover } else { t.toolbar },
            Stroke::new(1.0, if hovered { t.accent } else { t.border }),
            egui::StrokeKind::Inside,
        );
        // The shop's colour, as a swatch down the card's left edge.
        let swatch = Rect::from_min_size(card.left_top() + vec2(1.0, 1.0), vec2(6.0, card.height() - 2.0));
        ui.painter().rect_filled(swatch, egui::CornerRadius::ZERO, board.rgb());
        ui.painter().text(
            pos2(card.left() + 14.0, card.center().y - 8.0),
            Align2::LEFT_CENTER,
            &board.material,
            crate::fonts::semibold(t.font_button),
            t.text,
        );
        // The class is the part a person may not change, so it is the part the card leads with.
        let second = match board.kilograms() {
            Some(kilograms) => format!("{} · {kilograms:.1} kg", solidcraft_class(board)),
            None => format!("{} · {}", solidcraft_class(board), sheet_name(board)),
        };
        ui.painter().text(
            pos2(card.left() + 14.0, card.center().y + 9.0),
            Align2::LEFT_CENTER,
            second,
            crate::fonts::medium(t.font_small),
            t.text_dim,
        );
        let hint = match board.density_g_cm3 {
            Some(density) => format!("Insert a full sheet of {} ({} mm, {density} g/cm3)", board.material, board.size_label()),
            None => format!("Insert a full sheet of {} ({} mm) - no density, so it arrives as a colour", board.material, board.size_label()),
        };
        let response = response.on_hover_text(hint);
        if response.clicked() {
            insert_at = Some(board.clone());
        }
        crate::scenario::publish_target(&format!("ledger:board:{}", board.material), card);
        x += card_width + t.space_6;
    }

    // Reload sits after the boards, so it is reachable however many there are.
    let button = Rect::from_min_size(pos2(x.min(area.right() - 80.0), top), vec2(76.0, card_height.min(28.0)));
    let response = ui.interact(button, ui.id().with("ledger_reload"), Sense::click());
    ui.painter().rect(
        button,
        t.control_radius,
        if response.hovered() { t.hover } else { t.toolbar },
        Stroke::new(1.0, t.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(button.center(), Align2::CENTER_CENTER, "Reload", crate::fonts::semibold(t.font_small), t.text);
    if response.on_hover_text("Read Ledger's boards again").clicked() {
        reload = true;
    }
    if reload {
        boards(true);
    }
    if let Some(board) = insert_at {
        let _ = insert(app, &board);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shop's file, as Ledger writes it.
    #[test]
    fn a_published_board_is_read_with_its_size_colour_and_density() {
        let text = r##"{
            "format": "ledger-boards/1",
            "boards": [
                {"material": "Asian Night", "colour": "#424242", "width": 2743.0, "height": 1829.0,
                 "thickness": 18.0, "density_g_cm3": 0.7}
            ]
        }"##;
        let boards = parse(text).expect("the file reads");
        assert_eq!(boards.len(), 1);
        let board = &boards[0];
        assert_eq!(board.material, "Asian Night");
        assert_eq!(board.size_label(), "2743 × 1829 × 18");
        assert_eq!(board.rgb(), Color32::from_rgb(0x42, 0x42, 0x42));
        assert_eq!(board.density_g_cm3, Some(0.7));
    }

    /// A board nobody weighed is a colour, not a guess: the density is simply absent.
    #[test]
    fn a_board_without_a_density_has_no_density() {
        let text = r##"{"format": "ledger-boards/1",
            "boards": [{"material": "Mystery", "colour": "#123456", "width": 2440, "height": 1220,
                        "thickness": 18, "density_g_cm3": null}]}"##;
        let boards = parse(text).expect("the file reads");
        assert_eq!(boards[0].density_g_cm3, None);
    }

    /// A sheet's weight, checked against the editor's own measurement of the body it makes.
    #[test]
    fn a_board_weighs_what_the_editor_measures() {
        let text = r##"{"format": "ledger-boards/1", "boards": [
            {"material": "Asian Night", "colour": "#424242", "width": 2743, "height": 1829, "thickness": 18,
             "density_g_cm3": 0.7}
        ]}"##;
        let board = &parse(text).expect("the file reads")[0];
        // inspect.measure reports 63213.53 g for the same box: 90 305 046 mm3 at 0.7 g/cm3.
        let kilograms = board.kilograms().expect("it has a density");
        assert!((kilograms - 63.213).abs() < 0.01, "63.2 kg, not {kilograms}");
    }

    /// A file that is not the shop's is refused rather than read as zero boards.
    #[test]
    fn another_file_is_refused() {
        assert!(parse(r#"{"boards": []}"#).is_err());
        assert!(parse("not json at all").is_err());
    }

    /// A board with no size cannot become a body, so it is left out rather than made wrong.
    #[test]
    fn a_board_with_no_size_is_skipped() {
        let text = r##"{"format": "ledger-boards/1", "boards": [
            {"material": "Nowhere", "colour": "#ffffff", "width": 0, "height": 100, "thickness": 18}
        ]}"##;
        assert!(parse(text).expect("the file reads").is_empty());
    }
}
