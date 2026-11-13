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
    /// The folder Ledger files this board in, when it files them: a place to look, not a property of the board.
    pub folder: Option<String>,
}

impl Board {
    /// What the body is called in the design: "Asian Night sheet".
    fn body_label(&self) -> String {
        format!("{} sheet", self.material)
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
    ledger_meta(app).get("boards").and_then(serde_json::Value::as_object).cloned().unwrap_or_default()
}

/// Everything this tab keeps in the design under `shop.ledger`: the boards' limits and the turned grain.
///
/// `document.metadata.set` replaces the key's whole object, so every writer reads this, changes its own part and writes it
/// all back - otherwise recording a board would wipe the grain, and turning the grain would wipe the boards.
pub(crate) fn ledger_meta(app: &mut SolveApp) -> serde_json::Map<String, serde_json::Value> {
    // Another tool's data comes back under `metadata`; `properties` holds only SolidCraft's own text fields (author,
    // description, part_number, …) and answers null for this key, which would leave the guard blind.
    app.session
        .execute("document.metadata", &json!({}))
        .ok()
        .and_then(|answer| answer.get("metadata")?.get(KEY)?.as_object().cloned())
        .unwrap_or_default()
}

/// Set one entry under `shop.ledger`, keeping every other: the hole list beside the boards and the turned grain.
pub(crate) fn set_ledger_entry(app: &mut SolveApp, key: &str, value: serde_json::Value) {
    let mut meta = ledger_meta(app);
    meta.insert(key.to_owned(), value);
    let _ = app.run("document.metadata.set", json!({ "values": { KEY: meta } }));
}

/// Carry the design's records across renames: a board's sheet limits and its turned grain are both kept by body name, so
/// without this Name parts quietly dropped every board's limits and turned every turned board back.
pub(crate) fn follow_renames(app: &mut SolveApp, renamed: &[(String, String)]) {
    let meta = ledger_meta(app);
    if meta.is_empty() {
        return;
    }
    let moved = renamed_records(meta, renamed);
    let _ = app.run("document.metadata.set", json!({ "values": { KEY: moved } }));
}

/// The records with every renamed body's entries under its new name; entries for bodies not renamed stay as they were.
fn renamed_records(
    mut meta: serde_json::Map<String, serde_json::Value>,
    renamed: &[(String, String)],
) -> serde_json::Map<String, serde_json::Value> {
    let new_of = |name: &str| renamed.iter().find(|(old, _)| old == name).map(|(_, new)| new.clone());
    if let Some(boards) = meta.get("boards").and_then(serde_json::Value::as_object).cloned() {
        let mut out = serde_json::Map::new();
        // The ones staying first, the renamed after: a renamed board's record wins any stale entry under its new name.
        for (name, tag) in boards.iter().filter(|(name, _)| new_of(name).is_none()) {
            out.insert(name.clone(), tag.clone());
        }
        for (name, tag) in &boards {
            if let Some(new) = new_of(name) {
                out.insert(new, tag.clone());
            }
        }
        meta.insert("boards".to_owned(), serde_json::Value::Object(out));
    }
    if let Some(list) = meta.get(GRAIN_KEY).and_then(serde_json::Value::as_array).cloned() {
        let turned: std::collections::BTreeSet<String> = list
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(|name| new_of(name).unwrap_or_else(|| name.to_owned()))
            .collect();
        meta.insert(GRAIN_KEY.to_owned(), json!(turned.into_iter().collect::<Vec<_>>()));
    }
    // The hole list names each hole's board: a renamed board keeps its holes.
    if let Some(holes) = meta.get_mut("bores").and_then(|bores| bores.get_mut("holes")).and_then(serde_json::Value::as_array_mut) {
        for hole in holes {
            if let Some(new) = hole.get("board").and_then(serde_json::Value::as_str).and_then(new_of) {
                hole["board"] = json!(new);
            }
        }
    }
    meta
}

/// Record a board's limits in the design, under the name of the body that carries it.
///
/// `width`/`height` are what was cut; `max_width`/`max_height` are the sheet it came from, which is the cap that matters: a
/// panel cut at 720 x 450 may still grow to 1000 x 450, because the sheet allows it.
fn record(app: &mut SolveApp, name: &str, board: &Board, cut: (f64, f64)) -> Result<(), String> {
    let mut boards = design_boards(app);
    boards.insert(
        name.to_owned(),
        json!({
            "material": board.material,
            "width": cut.0,
            "height": cut.1,
            "max_width": board.width.max(board.height),
            "max_height": board.width.min(board.height),
            "thickness": board.thickness,
            "class": solidcraft_class(board),
            "sheet": sheet_name(board),
        }),
    );
    let mut meta = ledger_meta(app);
    meta.insert("boards".to_owned(), serde_json::Value::Object(boards));
    app.run("document.metadata.set", json!({ "values": { KEY: meta } }))?;
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
        if let Some(why) = complaint(tag, measured_size(body)?) {
            return Some(format!("{name} {why}"));
        }
    }
    None
}

/// A measured body's extent as `[thickness, short side, long side]`.
///
/// Sorted, so the smallest dimension is the thickness however the board is turned and the other two are its face.
fn measured_size(body: &serde_json::Value) -> Option<[f64; 3]> {
    let bbox = body.get("bbox")?;
    let min = bbox.get("min")?.as_array()?;
    let max = bbox.get("max")?.as_array()?;
    let mut size: Vec<f64> = (0..3)
        .map(|axis| {
            max.get(axis).and_then(serde_json::Value::as_f64).unwrap_or(0.0) - min.get(axis).and_then(serde_json::Value::as_f64).unwrap_or(0.0)
        })
        .collect();
    size.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some([size[0], size[1], size[2]])
}

/// Why a board of this size is no longer stock, or nothing when it is.
///
/// `size` is `[thickness, short side, long side]`. A board may be cut smaller than its sheet and never bigger; its thickness
/// is one of the shop's four classes and never anything else.
fn complaint(tag: &serde_json::Value, size: [f64; 3]) -> Option<String> {
    let thickness = size[0];
    let wanted = tag.get("thickness").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    if wanted > 0.0 && (thickness - wanted).abs() > SLACK_MM {
        return Some(format!(
            "is a {} board: its thickness cannot become {thickness:.1} mm",
            tag.get("class").and_then(serde_json::Value::as_str).unwrap_or("stock")
        ));
    }
    // The cap is the sheet, not the size the panel was cut at: a saw may take a smaller part from a bigger sheet, so a panel
    // cut at 720 x 450 can still be remade at 1000 x 450 before it is no longer stock at all.
    let cap = |key: &str, fallback: &str| {
        tag.get(key).and_then(serde_json::Value::as_f64).or_else(|| tag.get(fallback).and_then(serde_json::Value::as_f64)).unwrap_or(f64::INFINITY)
    };
    let (a, b) = (cap("max_width", "width"), cap("max_height", "height"));
    let (few_long, few_short) = if a >= b { (a, b) } else { (b, a) };
    if size[2] > few_long + SLACK_MM || size[1] > few_short + SLACK_MM {
        return Some(format!(
            "is cut from a {} sheet: {:.0} x {:.0} mm does not fit",
            tag.get("sheet").and_then(serde_json::Value::as_str).unwrap_or("stock"),
            size[2],
            size[1]
        ));
    }
    None
}

/// One row of the shop's hardware: a hinge, a handle, a connector. No geometry — the editor can only place what it has a
/// part file for, so this is a list to look at and pick from.
#[derive(Clone, Debug, PartialEq)]
pub struct Hardware {
    pub kind: String,
    pub name: String,
    pub model: String,
    pub price: f64,
}

/// What the shop's hardware file holds.
pub fn parse_hardware(text: &str) -> Result<Vec<Hardware>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if value.get("format").and_then(serde_json::Value::as_str) != Some("ledger-hardware/1") {
        return Err("not a Ledger hardware file".to_owned());
    }
    let entries = value.get("hardware").and_then(serde_json::Value::as_array).ok_or("no hardware in the file")?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let name = entry.get("name").and_then(serde_json::Value::as_str)?.to_owned();
            Some(Hardware {
                kind: entry.get("kind").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned(),
                name,
                model: entry.get("model").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned(),
                price: entry.get("price").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
            })
        })
        .collect())
}

/// The shop's hardware, read again when the file's timestamp moves.
pub fn hardware(reload: bool) -> (Vec<Hardware>, Option<String>) {
    let path = file().with_file_name("hardware.json");
    let stamp = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
    {
        let cache = hardware_cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !reload && stamp.is_some() && stamp == cache.stamp {
            return (cache.rows.clone(), cache.error.clone());
        }
    }
    let mut cache = hardware_cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match std::fs::read_to_string(&path) {
        Ok(text) => match parse_hardware(&text) {
            Ok(rows) => {
                cache.stamp = stamp;
                cache.rows = rows.clone();
                cache.error = None;
                (rows, None)
            }
            Err(error) => {
                cache.stamp = stamp;
                cache.rows.clear();
                cache.error = Some(format!("{}: {error}", path.display()));
                (Vec::new(), cache.error.clone())
            }
        },
        Err(error) => {
            cache.stamp = None;
            cache.rows.clear();
            cache.error = Some(format!("{}: {error}", path.display()));
            (Vec::new(), cache.error.clone())
        }
    }
}

#[derive(Default)]
struct HardwareCache {
    stamp: Option<SystemTime>,
    rows: Vec<Hardware>,
    error: Option<String>,
}

fn hardware_cache() -> &'static std::sync::Mutex<HardwareCache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HardwareCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HardwareCache::default()))
}

/// The folder Ledger keeps a design's saved geometry in: beside its database, next to the boards.
pub fn designs_dir() -> std::path::PathBuf {
    file().with_file_name("designs")
}

/// Save this design into Ledger's folder instead of asking about it.
///
/// The shop's ask: no save prompt, because Ledger is the record. A design Ledger opened and the operator changed is written
/// beside Ledger's database, named by the item's code, and closing asks nothing. Only a design Ledger owns is taken that way:
/// one with no file of its own, or one already living in this folder. A design the operator saved somewhere else still asks,
/// because that file is theirs. Started only for Ledger's own editor (`LEDGER_OWNED`), so a standalone SolidCraft keeps its
/// normal behaviour.
pub fn save_for_ledger(app: &mut SolveApp) -> bool {
    if std::env::var_os("LEDGER_OWNED").is_none() || !app.session.is_dirty() {
        return false;
    }
    let dir = designs_dir();
    let owned = match app.session.path.as_deref() {
        None => true,
        Some(path) => std::path::Path::new(path).parent().is_some_and(|parent| parent == dir),
    };
    if !owned {
        return false;
    }
    let Some((code, _)) = open_item() else {
        return false;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let path = dir.join(format!("{}.solidcraft", stem_of(&code)));
    let params = serde_json::json!({ "path": path.to_string_lossy(), "overwrite": true });
    match app.session.execute("file.save_as", &params) {
        Ok(_) => {
            app.session.mark_saved();
            true
        }
        Err(_) => false,
    }
}

/// A code as a file name: the same rule the picture uses, so a design and its picture sort together.
fn stem_of(code: &str) -> String {
    code.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}

/// The item Ledger last opened, as `(code, name)`: the picture taken here is filed against its code.
pub fn open_item() -> Option<(String, String)> {
    let path = file().with_file_name("current.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    if value.get("format").and_then(serde_json::Value::as_str) != Some("ledger-current/1") {
        return None;
    }
    let code = value.get("code").and_then(serde_json::Value::as_str)?.to_owned();
    let name = value.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    Some((code, name))
}

/// Take a picture of the model and file it as the open item's preview in Ledger.
///
/// `ui.render` draws the model as the viewport shows it, without needing a presented frame, which is what makes this work
/// whether or not the window is in front.
pub fn picture(app: &mut SolveApp) -> Result<String, String> {
    let Some((code, name)) = open_item() else {
        return Err("Ledger has no item open - open one from its Home tab first".to_owned());
    };
    let path = file().with_file_name("shots").join(format!("{}.png", stem_of(&code)));
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| format!("could not make {}: {error}", folder.display()))?;
    }
    // Drawn the way the channel's `ui.render` draws it, but in this process: that is a control *method*, not a command, so
    // `app.run` cannot reach it.
    let guides = crate::control::window_guides(app);
    let scene = crate::control::view_scene(app, guides);
    let png = solidcraft_engine::capture::png(&scene, &app.cam, 1200, 900).map_err(|error| format!("render failed: {error}"))?;
    std::fs::write(&path, &png.bytes).map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(format!("picture saved for {name} ({code})"))
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
    // `/1` and `/2` both read: `/2` adds the folder a board is filed in. A file with no marker at all is still refused.
    let format = value.get("format").and_then(serde_json::Value::as_str).unwrap_or_default();
    if !matches!(format, "ledger-boards/1" | "ledger-boards/2") {
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
            folder: entry.get("folder").and_then(serde_json::Value::as_str).map(str::to_owned).filter(|folder| !folder.is_empty()),
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
    let mut n = 2;
    while taken(app, &format!("{base} ({n})")) {
        n += 1;
    }
    format!("{base} ({n})")
}

/// Put one board into the design, and answer the body's name.
///
/// The order matters: the physical material exists first (SolidCraft assigns by name), then the body, then its look, then
/// the material itself, so a failure leaves a coloured board rather than a board with a material that is not there.
pub fn insert(app: &mut SolveApp, board: &Board, cut: (f64, f64)) -> Result<String, String> {
    let (width, height) = cut;
    let colour = format!("#{:02x}{:02x}{:02x}", board.rgb().r(), board.rgb().g(), board.rgb().b());
    if let Some(density) = board.density_g_cm3 {
        let listed = app.run("material.list", json!({}))?;
        if !listed_has(&listed, &board.material) {
            app.run("material.create", json!({ "name": board.material, "density_g_cm3": density, "color": colour }))?;
        }
    }
    let name = free_name(app, &panel_label(board, cut));
    // Lay the board flat beyond whatever is already there, as sheets are laid out on a bench, rather than inside it.
    let x = bench_end(app)?;
    app.run(
        "solid.box",
        json!({
            "length": width,
            "width": height,
            "height": board.thickness,
            "center": [x + width / 2.0, height / 2.0, board.thickness / 2.0],
            "name": name,
            "body_name": name,
        }),
    )?;
    app.run("appearance.assign", json!({ "bodies": [name], "color": colour }))?;
    if board.density_g_cm3.is_some() {
        app.run("material.assign", json!({ "bodies": [name], "material": board.material }))?;
    }
    // The design keeps the board's own limits, so the guard still knows them after a save and a reopen.
    record(app, &name, board, cut)?;
    app.fit_view();
    Ok(name)
}

/// What the body is called: a whole sheet keeps its board's name, a cut panel says its size.
fn panel_label(board: &Board, cut: (f64, f64)) -> String {
    let long = board.width.max(board.height);
    let short = board.width.min(board.height);
    let whole = |a: f64, b: f64| (a - long).abs() <= 0.5 && (b - short).abs() <= 0.5;
    if whole(cut.0, cut.1) || whole(cut.1, cut.0) { board.body_label() } else { format!("{} {}x{}", board.material, mm(cut.0), mm(cut.1)) }
}

/// A panel's size, or nothing when it is not something a saw could cut from this board.
///
/// Width is along the sheet's long side, height across it. The thickness is never asked for: it is the board's.
fn cut_size(board: &Board, width: f64, height: f64) -> Option<(f64, f64)> {
    let long = board.width.max(board.height);
    let short = board.width.min(board.height);
    (width > 0.0 && height > 0.0 && width <= long + 0.5 && height <= short + 0.5).then_some((width, height))
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

/// Whether the model is drawn in wireframe, so the fitting markers inside the panels stay in sight. The shop asked for this
/// switch **here** rather than in Ledger: swapping between the shaded and the edge-only view is a thing done while working, not
/// a preference of the host program.
/// The Ledger tab's **edges** view is on: boards are drawn as outlines and hardware stays solid in its colours. Only this
/// button sets it; any other way out of wireframe ends it (the viewport clears it), so SolidCraft's own wireframe stays
/// plain outlines.
pub(crate) static WIREFRAME_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the viewport should draw hardware solid in wireframe right now.
pub(crate) fn solid_hardware(app: &SolveApp) -> bool {
    if app.ui.visual_style != 2 {
        // Left wireframe some other way: the next wireframe, from anywhere but this button, is the plain one.
        WIREFRAME_ON.store(false, std::sync::atomic::Ordering::Relaxed);
        return false;
    }
    WIREFRAME_ON.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the grain arrows are drawn. A way of looking at the design, so it is the session's, not the design's.
static GRAIN_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Under `shop.ledger` in the design's metadata: the boards whose grain runs **across** them rather than along their long
/// side. Kept in the design, so a turned board stays turned after a save and a reopen, travels with the file, and a turn is
/// one undo step like any other edit.
const GRAIN_KEY: &str = "grain_across";

/// The first version drew the grain as strip bodies named like this. They are swept away when the grain is shown.
const GRAIN_PREFIX: &str = "grain ";

fn is_grain(name: &str) -> bool {
    name.starts_with(GRAIN_PREFIX)
}

/// Take the first version's strip bodies out of a design that still carries them.
fn clear_grain(app: &mut SolveApp) {
    let names: Vec<String> =
        app.session.world_state().bodies.iter().map(|body| body.name.clone()).filter(|name| is_grain(name)).collect();
    if !names.is_empty() {
        let _ = app.run("solid.remove", json!({ "bodies": names }));
    }
}

/// One board the grain is drawn on: its box, and whether its grain is turned across it.
#[derive(Clone, Debug, PartialEq)]
pub struct GrainBoard {
    pub name: String,
    pub lo: [f64; 3],
    pub hi: [f64; 3],
    pub across: bool,
}

impl GrainBoard {
    fn size(&self) -> [f64; 3] {
        [self.hi[0] - self.lo[0], self.hi[1] - self.lo[1], self.hi[2] - self.lo[2]]
    }

    /// Thin, across, along: the axes of the box from shortest to longest.
    fn axes(&self) -> [usize; 3] {
        let size = self.size();
        let mut axes = [0usize, 1, 2];
        axes.sort_by(|a, b| size[*a].total_cmp(&size[*b]));
        axes
    }

    /// The axis the grain runs along: the long side, or the short one when the shop has turned it.
    pub fn run_axis(&self) -> usize {
        let [_, across, along] = self.axes();
        if self.across { across } else { along }
    }
}

/// Whether a box is a board the grain means something on: sheet-thick, and wide enough to have a face. A cam block, a glide
/// or a leg post is not.
fn is_board_box(size: [f64; 3]) -> bool {
    let mut sorted = size;
    sorted.sort_by(f64::total_cmp);
    (6.0..=40.0).contains(&sorted[0]) && sorted[1] >= 50.0
}

/// The boxes of every body in a measurement, by name, leaving out the old strip bodies.
fn measured_boxes(answer: &serde_json::Value) -> Vec<(String, [f64; 3], [f64; 3])> {
    let Some(bodies) = answer.get("bodies").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    bodies
        .iter()
        .filter_map(|body| {
            let name = body.get("name")?.as_str()?;
            if is_grain(name) {
                return None;
            }
            let bbox = body.get("bbox")?;
            let corner = |key: &str| -> Option<[f64; 3]> {
                let list = bbox.get(key)?.as_array()?;
                Some([list.first()?.as_f64()?, list.get(1)?.as_f64()?, list.get(2)?.as_f64()?])
            };
            Some((name.to_owned(), corner("min")?, corner("max")?))
        })
        .collect()
}

/// The boards of a measurement, each with the direction the design records for it.
pub fn grain_boards(answer: &serde_json::Value, across: &std::collections::BTreeSet<String>) -> Vec<GrainBoard> {
    measured_boxes(answer)
        .into_iter()
        .filter(|(_, lo, hi)| is_board_box([hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]))
        .map(|(name, lo, hi)| GrainBoard { across: across.contains(&name), name, lo, hi })
        .collect()
}

/// The boards whose grain the design records as turned.
fn turned_in(meta: &serde_json::Map<String, serde_json::Value>) -> std::collections::BTreeSet<String> {
    meta.get(GRAIN_KEY)
        .and_then(serde_json::Value::as_array)
        .map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// What the arrows are drawn from, read once per change of the design rather than on every frame.
#[derive(Default)]
struct GrainCache {
    /// The design and the revision this was read at.
    key: Option<(u64, u64, usize)>,
    boards: Vec<GrainBoard>,
    /// Every body's box, boards or not: what can stand between the eye and an arrow.
    blockers: Vec<(String, [f64; 3], [f64; 3])>,
}

static GRAIN_CACHE: std::sync::Mutex<GrainCache> =
    std::sync::Mutex::new(GrainCache { key: None, boards: Vec::new(), blockers: Vec::new() });

fn design_key(app: &SolveApp) -> (u64, u64, usize) {
    (app.session.uid, app.session.revision, std::sync::Arc::as_ptr(&app.session.doc) as *const () as usize)
}

/// Read the boards again when the design has changed since the last read: a board moved, merged, renamed, a design opened
/// or switched. The first version measured once, when the menu was pressed, which is why its arrows went missing or
/// stood where a board used to be.
fn refresh_grain(app: &mut SolveApp) {
    let key = design_key(app);
    if GRAIN_CACHE.lock().map(|cache| cache.key == Some(key)).unwrap_or(true) {
        return;
    }
    // Both reads are queries: they change nothing and add nothing to the undo stack.
    let measured = app.session.execute("inspect.measure", &json!({})).unwrap_or_default();
    let meta = ledger_meta(app);
    let across = turned_in(&meta);
    let boards = grain_boards(&measured, &across);
    let blockers = measured_boxes(&measured);
    if let Ok(mut cache) = GRAIN_CACHE.lock() {
        // Keyed after the reads, so a read that bumped the revision cannot make this run on every frame.
        cache.key = Some(design_key(app));
        cache.boards = boards;
        cache.blockers = blockers;
    }
}

/// The boards a selection points at: a picked body, or the body a picked face, edge or corner is on.
fn selected_boards(app: &SolveApp) -> Vec<String> {
    let boards: Vec<String> =
        GRAIN_CACHE.lock().map(|cache| cache.boards.iter().map(|b| b.name.clone()).collect()).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for sel in &app.session.selection {
        let name = match sel {
            solidcraft_engine::Sel::Body { name } => name,
            solidcraft_engine::Sel::Face { body, .. } | solidcraft_engine::Sel::Edge { body, .. } => body,
            solidcraft_engine::Sel::Vertex { body, .. } => body,
            _ => continue,
        };
        if boards.contains(name) && !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
}

/// Turn the grain on these boards - each one on its own, so a mixed selection swaps each - or put every board back along
/// its long side when `names` is `None`. One metadata write: one undo step.
fn turn_grain(app: &mut SolveApp, names: Option<&[String]>) -> Result<usize, String> {
    let mut meta = ledger_meta(app);
    let mut across = turned_in(&meta);
    let changed = match names {
        Some(names) => {
            for name in names {
                if !across.remove(name) {
                    across.insert(name.clone());
                }
            }
            names.len()
        }
        None => {
            let count = across.len();
            across.clear();
            count
        }
    };
    meta.insert(GRAIN_KEY.to_owned(), json!(across.into_iter().collect::<Vec<_>>()));
    app.run("document.metadata.set", json!({ "values": { KEY: meta } }))?;
    Ok(changed)
}

/// Where a ray meets a box, as a distance along it, if it does.
fn ray_hits_box(origin: [f64; 3], dir: [f64; 3], lo: [f64; 3], hi: [f64; 3]) -> Option<f64> {
    let (mut near, mut far) = (f64::NEG_INFINITY, f64::INFINITY);
    for axis in 0..3 {
        if dir[axis].abs() < 1e-12 {
            if origin[axis] < lo[axis] || origin[axis] > hi[axis] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[axis] - origin[axis]) / dir[axis], (hi[axis] - origin[axis]) / dir[axis]);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    (near <= far && far >= 0.0).then_some(near.max(0.0))
}

/// Which face of a board the arrow goes on, seen from `look`: the broad face turned towards the eye. Returns the point at
/// the middle of that face (a hair proud of it) and its outward normal, or `None` when the board is seen edge-on.
fn facing_side(board: &GrainBoard, look: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    let [thin, _, _] = board.axes();
    let mut centre: [f64; 3] = std::array::from_fn(|axis| (board.lo[axis] + board.hi[axis]) / 2.0);
    // The face whose outward normal points back at the eye.
    let sign = if look[thin] > 0.0 { -1.0 } else { 1.0 };
    if look[thin].abs() < 0.05 {
        return None;
    }
    centre[thin] = if sign > 0.0 { board.hi[thin] + 0.5 } else { board.lo[thin] - 0.5 };
    let mut normal = [0.0; 3];
    normal[thin] = sign;
    Some((centre, normal))
}

/// A two-way arrow on each board's face that is turned towards the eye, unless another body stands in front of it. Painted
/// over the model rather than modelled, so it stays crisp at any zoom; the depth test is done here against the boxes, which
/// is exact for boards.
///
/// While boards are selected, theirs are drawn bold and the rest faded, so it is plain which boards a turn will change.
pub fn paint_grain(app: &mut SolveApp, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    if !GRAIN_ON.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    refresh_grain(app);
    let selected = selected_boards(app);
    let hidden = app.session.visibility.hidden_bodies.clone();
    let Ok(cache) = GRAIN_CACHE.lock() else { return };
    let red = egui::Color32::from_rgb(240, 40, 40);
    let v3 = |p: [f64; 3]| solidcraft_engine::geom::Vec3::new(p[0], p[1], p[2]);
    for board in &cache.boards {
        if hidden.contains(&board.name) {
            continue;
        }
        let middle: [f64; 3] = std::array::from_fn(|axis| (board.lo[axis] + board.hi[axis]) / 2.0);
        let Some(mid) = proj.to_screen(v3(middle)) else { continue };
        let (_, look) = proj.ray(mid);
        let Some((centre, _normal)) = facing_side(board, [look.x, look.y, look.z]) else { continue };
        let run = board.run_axis();
        let [thin, _, _] = board.axes();
        let size = board.size();
        // Whether a point on the face can be seen: the first thing the eye's ray meets on the way to it is this board.
        let seen = |p: [f64; 3]| -> bool {
            let Some(at) = proj.to_screen(v3(p)) else { return false };
            let (origin, dir) = proj.ray(at);
            let (origin, dir) = ([origin.x, origin.y, origin.z], [dir.x, dir.y, dir.z]);
            let Some(to_face) = ray_hits_box(origin, dir, board.lo, board.hi) else { return false };
            !cache.blockers.iter().any(|(name, lo, hi)| {
                name != &board.name && !hidden.contains(name) && ray_hits_box(origin, dir, *lo, *hi).is_some_and(|t| t < to_face - 0.5)
            })
        };
        // Where on the face the arrow stands, and how long it is: along the grain through the middle of the face when all
        // of that shows, else the longest stretch of a nearby line that does - a modesty seen from behind is half under the
        // top, and its arrow belongs on the half in sight, not on the top, and not nowhere.
        let [u, v] = {
            let mut others = (0..3).filter(|axis| *axis != thin);
            [others.next().unwrap_or(0), others.next().unwrap_or(1)]
        };
        let across_axis = if u == run { v } else { u };
        let full = size[run] * 0.34;
        const SAMPLES: usize = 24;
        // The longest unbroken visible stretch of the grain line through `through`, as two points on it.
        let stretch = |through: [f64; 3]| -> Option<([f64; 3], [f64; 3], f64)> {
            let at = |k: usize| {
                let mut p = through;
                p[run] = middle[run] - size[run] * 0.45 + size[run] * 0.9 * k as f64 / SAMPLES as f64;
                p
            };
            let shown: Vec<bool> = (0..=SAMPLES).map(|k| seen(at(k))).collect();
            let (mut best, mut from) = (None::<(usize, usize)>, None::<usize>);
            for (k, on) in shown.iter().chain(std::iter::once(&false)).enumerate() {
                match (on, from) {
                    (true, None) => from = Some(k),
                    (false, Some(f)) => {
                        if best.is_none_or(|(a, b)| k - 1 - f > b - a) {
                            best = Some((f, k - 1));
                        }
                        from = None;
                    }
                    _ => {}
                }
            }
            let (a, b) = best?;
            let (pa, pb) = (at(a), at(b));
            let length = (pb[run] - pa[run]).abs();
            // No longer than the arrow on an open face, centred in the stretch, and a little in from its ends.
            let mid_run = (pa[run] + pb[run]) / 2.0;
            let half = (length / 2.0 * 0.9).min(full);
            let (mut ea, mut eb) = (pa, pb);
            ea[run] = mid_run - half;
            eb[run] = mid_run + half;
            Some((ea, eb, half))
        };
        let lines: [f64; 5] = [0.0, 0.2, -0.2, 0.35, -0.35];
        let mut found: Option<([f64; 3], [f64; 3], f64)> = None;
        for offset in lines {
            let mut through = centre;
            through[across_axis] = middle[across_axis] + size[across_axis] * offset;
            if let Some(candidate) = stretch(through) {
                let good = candidate.2 >= full * 0.6;
                if found.as_ref().is_none_or(|f| candidate.2 > f.2) {
                    found = Some(candidate);
                }
                if good {
                    break;
                }
            }
        }
        // Hidden behind another body: the arrow would float over the board in front and say the wrong thing about it.
        let Some((end_a, end_b, _)) = found else { continue };
        let (Some(a), Some(b)) = (proj.to_screen(v3(end_a)), proj.to_screen(v3(end_b))) else { continue };
        if (b - a).length() < 18.0 {
            continue;
        }
        let (colour, width) = if selected.is_empty() {
            (red, 3.0)
        } else if selected.contains(&board.name) {
            (egui::Color32::from_rgb(255, 70, 70), 4.0)
        } else {
            (red.gamma_multiply(0.35), 2.0)
        };
        two_way_arrow(painter, a, b, colour, width);
    }
}

/// A line with a head at each end: the grain runs both ways along a board, so the arrow says "this direction", not "that way".
fn two_way_arrow(painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2, col: egui::Color32, shaft: f32) {
    let v = b - a;
    let length = v.length();
    if length < 1.0 {
        return;
    }
    let u = v / length;
    let n = egui::vec2(-u.y, u.x);
    let head = (14.0 + shaft * 1.5).min(length * 0.45);
    painter.line_segment([a + u * head * 0.5, b - u * head * 0.5], egui::Stroke::new(shaft, col));
    for (tip, back) in [(a, u), (b, -u)] {
        painter.add(egui::Shape::convex_polygon(
            vec![tip, tip + back * head - n * head * 0.5, tip + back * head + n * head * 0.5],
            col,
            egui::Stroke::NONE,
        ));
    }
}

/// Whether a move should be followed by closing the gap it left, set by the tab's check box.
static SNAP_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether a snap is already running, so the move it makes does not ask for another.
static SNAPPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How close two faces must be before a move is closed onto one of them.
const SNAP_MM: f64 = 10.0;

/// The move that puts `moving` flush against `target`, or nothing when no pair of faces is within `tolerance`.
///
/// Each board is `(min, max)` in millimetres. A candidate is one board's face a little `tolerance` away from the other's
/// opposing face, with the two faces overlapping by at least a quarter of the smaller one in both other directions - a board
/// touching a corner is not resting on it. The smallest gap wins, and the answer moves the board along that one axis only: no
/// centring, because a board that jumps sideways when it is set down is worse than one that does not move at all.
fn snap_translation(moving: ([f64; 3], [f64; 3]), target: ([f64; 3], [f64; 3]), tolerance: f64) -> Option<[f64; 3]> {
    let mut best: Option<(f64, f64, [f64; 3])> = None;
    for axis in 0..3 {
        let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
        // The faces have to overlap across the other two axes, or this is a corner meeting an edge.
        let overlap = |other: usize| {
            let shared = moving.1[other].min(target.1[other]) - moving.0[other].max(target.0[other]);
            let smaller = (moving.1[other] - moving.0[other]).min(target.1[other] - target.0[other]);
            shared >= 0.25 * smaller.max(1.0)
        };
        if !(overlap(b) && overlap(c)) {
            continue;
        }
        // Two ways to face each other along one axis: moving below the target, or above it.
        for (gap, step) in [(target.0[axis] - moving.1[axis], 1.0), (moving.0[axis] - target.1[axis], -1.0)] {
            if !(0.0..=tolerance).contains(&gap) {
                continue;
            }
            let mut translate = [0.0; 3];
            translate[axis] = gap * step;
            let overlap_total =
                (moving.1[b].min(target.1[b]) - moving.0[b].max(target.0[b])) + (moving.1[c].min(target.1[c]) - moving.0[c].max(target.0[c]));
            let better = best.is_none_or(|(fewest, most, _)| gap < fewest - 1e-9 || ((gap - fewest).abs() <= 1e-9 && overlap_total > most));
            if better {
                best = Some((gap, overlap_total, translate));
            }
        }
    }
    best.map(|(_, _, translate)| translate)
}

/// Close the gap a board was just moved next to, when snapping is on.
///
/// Only a move is followed, and only one: the board closest to another face is set flush against it and the operator is told.
/// Boards stay separate bodies, which is what the audit counts.
pub fn snap_after_move(app: &mut SolveApp, command: &str, params: &serde_json::Value) {
    use std::sync::atomic::Ordering;
    if command != "solid.move" || !SNAP_ON.load(Ordering::Relaxed) {
        return;
    }
    if params.get("copy").and_then(serde_json::Value::as_bool) == Some(true) || params.get("translate").is_none() {
        return;
    }
    if SNAPPING.swap(true, Ordering::SeqCst) {
        return;
    }
    // Hardware moved with snap on slides along its joint: whatever the move did off the joint's line is taken back.
    let moved_names: Vec<String> = params
        .get("bodies")
        .and_then(serde_json::Value::as_array)
        .map(|list| list.iter().filter_map(serde_json::Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let back = crate::ledger_hand::snap_moved(app, &moved_names);
    if !back.is_empty() {
        app.set_status(format!("snap: {} kept on {} joint", back.join(", "), if back.len() == 1 { "its" } else { "their" }), false);
    }
    let boards = design_boards(app);
    let moved: Vec<String> = params
        .get("bodies")
        .and_then(serde_json::Value::as_array)
        .map(|list| list.iter().filter_map(serde_json::Value::as_str).filter(|name| boards.contains_key(*name)).map(str::to_owned).collect())
        .unwrap_or_default();
    if !moved.is_empty()
        && boards.len() > moved.len()
        && let Some((name, translate)) = nearest_snap(app, &boards, &moved)
        && app.run("solid.move", json!({ "bodies": [name], "translate": translate })).is_ok()
    {
        app.set_status(format!("snapped {name} flush"), false);
    }
    SNAPPING.store(false, Ordering::SeqCst);
}

/// The board that is nearest to touching another, and the move that closes the gap.
fn nearest_snap(app: &mut SolveApp, boards: &serde_json::Map<String, serde_json::Value>, moved: &[String]) -> Option<(String, [f64; 3])> {
    let names: Vec<String> = boards.keys().cloned().collect();
    let measured = app.run("inspect.measure", json!({ "bodies": names })).ok()?;
    let mut boxes: Vec<(String, ([f64; 3], [f64; 3]))> = Vec::new();
    for body in measured.get("bodies")?.as_array()? {
        let name = body.get("name")?.as_str()?.to_owned();
        let bbox = body.get("bbox")?;
        let min: [f64; 3] = array3(bbox.get("min")?)?;
        let max: [f64; 3] = array3(bbox.get("max")?)?;
        boxes.push((name, (min, max)));
    }
    let mut best: Option<(f64, String, [f64; 3])> = None;
    for (name, shape) in &boxes {
        if !moved.contains(name) {
            continue;
        }
        for (other, target) in &boxes {
            if other == name || moved.contains(other) {
                continue;
            }
            let Some(translate) = snap_translation(*shape, *target, SNAP_MM) else { continue };
            let distance = translate.iter().map(|step| step.abs()).sum::<f64>();
            if best.as_ref().is_none_or(|(closest, _, _)| distance < *closest) {
                best = Some((distance, name.clone(), translate));
            }
        }
    }
    best.map(|(_, name, translate)| (name, translate))
}

/// Three numbers from a JSON array, or nothing when the answer is shaped differently.
fn array3(value: &serde_json::Value) -> Option<[f64; 3]> {
    let list = value.as_array()?;
    Some([list.first()?.as_f64()?, list.get(1)?.as_f64()?, list.get(2)?.as_f64()?])
}

/// The panel a person is about to cut, kept between frames so a half-typed size is not lost.
#[derive(Clone, Default, Debug, PartialEq)]
struct PanelForm {
    board: Option<String>,
    width: String,
    height: String,
}

impl PanelForm {
    /// A form for this board, filled with the whole sheet - the common case, and a size the saw can always cut.
    fn for_board(board: &Board) -> Self {
        let (long, short) = (board.width.max(board.height), board.width.min(board.height));
        Self { board: Some(board.material.clone()), width: mm(long), height: mm(short) }
    }

    /// What the fields say, as a size on this board, or why they do not say one.
    fn size(&self, board: &Board) -> Result<(f64, f64), String> {
        let number = |text: &str, axis: &str| text.trim().replace([',', ' '], "").parse::<f64>().map_err(|_| format!("{axis} is not a number"));
        let (width, height) = (number(&self.width, "width")?, number(&self.height, "height")?);
        cut_size(board, width, height).ok_or_else(|| {
            format!(
                "{width:.0} x {height:.0} mm does not come out of this board ({} x {})",
                mm(board.width.max(board.height)),
                mm(board.width.min(board.height))
            )
        })
    }
}

fn form_id() -> egui::Id {
    egui::Id::new("ledger_cut")
}

/// The board a panel is being cut from, while the panel form is open. A menu's pick is a one-frame event, so the choice has
/// to be kept - a window drawn from the event alone would appear for one frame and never be seen.
fn cut_board_id() -> egui::Id {
    egui::Id::new("sc_ledger_cut_board")
}

/// Which list the tab is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Subtab {
    Materials,
    Hardware,
}

fn subtab(ctx: &egui::Context) -> Subtab {
    match ctx.data(|data| data.get_temp::<u8>(egui::Id::new("ledger_sub"))).unwrap_or(0) {
        1 => Subtab::Hardware,
        _ => Subtab::Materials,
    }
}

fn set_subtab(ctx: &egui::Context, sub: Subtab) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("ledger_sub"), u8::from(sub == Subtab::Hardware)));
}



/// The shop's boards as the menu draws them: what nobody filed first, then one group per folder, in the order Ledger
/// published them. Inside a group, one entry per material; inside a material, its sizes with the longest side first.
#[allow(clippy::type_complexity)]
fn grouped(boards: &[Board]) -> Vec<(Option<String>, Vec<(String, Vec<Board>)>)> {
    let mut order: Vec<Option<String>> = Vec::new();
    for board in boards {
        if !order.contains(&board.folder) {
            order.push(board.folder.clone());
        }
    }
    // Unfiled boards are a list of materials at the top level, so they come first whatever order Ledger published them in.
    order.sort_by_key(|folder| folder.is_some());

    order
        .into_iter()
        .map(|folder| {
            let mut names: Vec<String> = Vec::new();
            for board in boards.iter().filter(|board| board.folder == folder) {
                if !names.contains(&board.material) {
                    names.push(board.material.clone());
                }
            }
            let materials = names
                .into_iter()
                .map(|name| {
                    let mut sizes: Vec<Board> = boards.iter().filter(|board| board.folder == folder && board.material == name).cloned().collect();
                    sizes.sort_by(|a, b| b.width.max(b.height).total_cmp(&a.width.max(a.height)));
                    (name, sizes)
                })
                .collect();
            (folder, materials)
        })
        .collect()
}

/// What one size row says: the sheet's size, the class the shop cuts it in, and its weight when someone has weighed it.
fn board_size_label(board: &Board) -> String {
    let size = format!("{} x {} · {}", mm(board.width.max(board.height)), mm(board.width.min(board.height)), solidcraft_class(board));
    match board.kilograms() {
        Some(kilograms) => format!("{size} · {kilograms:.1} kg"),
        None => size,
    }
}

// The ribbon's own metrics, copied from `toolbar.rs` where they are private. A group here has to sit in the row exactly like
// every other tab's: a button row 60 high, the group's name under it in a 20 px row, and a hairline after it.
const BUTTON_GAP: f32 = 3.0;
const BUTTON_HEIGHT: f32 = 60.0;
/// How far the label sits above a button's bottom edge when there is an icon over it.
const BUTTON_LABEL_BOTTOM: f32 = 8.0;
const BUTTON_MIN_WIDTH: f32 = 54.0;
const BUTTON_MAX_WIDTH: f32 = 76.0;
const BUTTON_LABEL_PADDING: f32 = 10.0;
const SECTION_HEIGHT: f32 = 20.0;
const INSET: f32 = 4.0;
const PANEL_GAP: f32 = 10.0;
/// The big buttons of this tab are squares: this tab's buttons are words, not icons, and a wide thin box reads as a chip
/// rather than as part of the ribbon. Narrow enough that `Picture` still fits, wide enough that `Board ▾` does.
const SQUARE: f32 = 74.0;
/// A stacked button's height: two of them fill the ribbon's button row exactly, the way a two-deep panel does elsewhere.
const STACKED_HEIGHT: f32 = 28.5;

/// How wide a button has to be for its label, in the ribbon's own range.
fn button_width(ui: &egui::Ui, label: &str) -> f32 {
    button_width_at(ui, label, BUTTON_MAX_WIDTH)
}

/// The same measurement with its own ceiling, for a label longer than the ribbon's usual two words.
fn button_width_at(ui: &egui::Ui, label: &str, ceiling: f32) -> f32 {
    let t = Tokens::get();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), crate::fonts::semibold(t.font_button), t.text);
    (galley.size().x + BUTTON_LABEL_PADDING).clamp(BUTTON_MIN_WIDTH, ceiling)
}

/// The words of a label, broken so each line fits `max_width`. A single word longer than the button is left whole rather than
/// cut - it is better to see it than to read an ellipsis.
fn break_label(
    painter: &egui::Painter,
    label: &str,
    font: &egui::FontId,
    max_width: f32,
) -> Vec<String> {
    let width = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    };
    if width(label) <= max_width {
        return vec![label.to_owned()];
    }
    let words: Vec<&str> = label.split_whitespace().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in words {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if width(&candidate) <= max_width || current.is_empty() {
            current = candidate;
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_owned();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// One button of the tab, drawn as the ribbon draws its own: a rounded box, the label centred, a chevron when it opens a menu.
fn ribbon_button(ui: &egui::Ui, id: &str, rect: Rect, label: &str, menu: bool, on: bool) -> egui::Response {
    ribbon_button_icon(ui, id, rect, label, menu, on, None)
}

/// One button with an icon over its label, which is how every large button in the ribbon is drawn: the picture at the top, the
/// word along the bottom. `None` keeps the label centred, for the short stacked buttons.
#[allow(clippy::too_many_arguments)]
fn ribbon_button_icon(
    ui: &egui::Ui,
    id: &str,
    rect: Rect,
    label: &str,
    menu: bool,
    on: bool,
    icon: Option<&str>,
) -> egui::Response {
    let t = Tokens::get();
    let response = ui.interact(rect, ui.id().with(("ledger_button", id)), Sense::click());
    // A pressed button reads in the tab's accent, the way the ribbon's own pressed buttons do - `selected_ink` is for a
    // button filled with the accent, which this is not.
    let ink = if on { t.accent } else { t.text };
    // A pressed button is a soft wash of the tab's accent, the way the ribbon's own pressed buttons are, not a bare hover grey.
    let fill = if on {
        t.hover.lerp_to_gamma(t.accent, 0.28)
    } else if response.hovered() {
        t.hover
    } else {
        t.toolbar
    };
    ui.painter().rect(rect, t.radius_sm, fill, Stroke::new(1.0, if on { t.accent } else { t.border }), egui::StrokeKind::Inside);
    // A label longer than the button breaks into lines that each fit, and every line is drawn centred. This is done by hand
    // rather than through a wrapping layout job: with a centre alignment that job returns a galley as wide as the wrap width,
    // and the words were drawn past the button's edge.
    let font = crate::fonts::semibold(t.font_small);
    let max_width = (rect.width() - BUTTON_LABEL_PADDING).max(24.0);
    let lines = break_label(ui.painter(), label, &font, max_width);
    let line_height = ui.painter().layout_no_wrap("Ag".to_owned(), font.clone(), ink).size().y;
    let block_height = line_height * lines.len() as f32;
    let mut line_y = match icon {
        Some(_) => rect.bottom() - BUTTON_LABEL_BOTTOM - block_height,
        None => rect.center().y - block_height / 2.0,
    };
    for line in &lines {
        let galley = ui.painter().layout_no_wrap(line.clone(), font.clone(), ink);
        ui.painter().galley(
            pos2(rect.center().x - galley.size().x / 2.0, line_y),
            galley,
            ink,
        );
        line_y += line_height;
    }
    let label_line_y = rect.bottom() - BUTTON_LABEL_BOTTOM - line_height;
    if let Some(icon) = icon {
        let side = 20.0;
        let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + 16.0), vec2(side, side));
        crate::icons::paint_icon(ui.painter(), icon_rect, icon, if on { 1.0 } else { 0.9 });
    }
    if menu {
        let arrow = Rect::from_center_size(
            pos2(rect.right() - 7.0, label_line_y + line_height / 2.0),
            vec2(10.0, 10.0),
        );
        crate::ui_kit::dropdown_indicator(ui.painter(), arrow, t.text_dim);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A group's name under its buttons, and the hairline that closes the group - the ribbon's own separator between sections.
fn group_caption(ui: &egui::Ui, area: Rect, left: f32, width: f32, caption: &str) {
    let t = Tokens::get();
    let row = Rect::from_min_size(
        pos2(left + INSET, area.top() + area.height() - SECTION_HEIGHT - (INSET - 1.0)),
        vec2((width - 2.0 * INSET).max(0.0), SECTION_HEIGHT),
    );
    let galley = ui
        .painter()
        .layout_no_wrap(caption.to_owned(), egui::FontId::proportional(t.font_caption), t.text_dim);
    let at = pos2(row.center().x - galley.size().x / 2.0, row.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, t.text_dim);
}

/// The hairline between two groups, drawn **once** at the boundary by the group that starts after it. Two groups each drawing
/// their own edge put two lines a pixel apart, which read as a doubled divider.
fn group_rule(ui: &egui::Ui, area: Rect, x: f32) {
    let t = Tokens::get();
    ui.painter().line_segment(
        [pos2(x - PANEL_GAP * 0.5, area.top() + 2.0), pos2(x - PANEL_GAP * 0.5, area.bottom() - 1.0)],
        t.tertiary_border(t.border),
    );
}

/// The `Board ▾` button and its menu: folders, then materials, then sizes. Picking a size answers the board to add as a whole
/// sheet; the last row of a material offers the panel form instead.
fn board_dropdown(ui: &mut egui::Ui, rect: Rect, groups: &[(Option<String>, Vec<(String, Vec<Board>)>)]) -> (Option<Board>, Option<Board>) {
    let mut add: Option<Board> = None;
    let mut cut: Option<Board> = None;
    let response = ribbon_button_icon(ui, "board", rect, "Board", true, false, Some("sheet"));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let response = ribbon_button_icon(ui, "board", rect, "Board", true, open, Some("sheet"));
    crate::scenario::publish_handle("ledger:board_menu", rect.center());

    // Sizes are leaves; a material opens them beside its row, and a folder opens the materials.
    let list = |ui: &mut egui::Ui, materials: &[(String, Vec<Board>)], add: &mut Option<Board>, cut: &mut Option<Board>| {
        for (name, sizes) in materials {
            crate::ui_kit::Menu::button(ui, name, |ui| {
                for board in sizes {
                    if ui.selectable_label(false, board_size_label(board)).clicked() {
                        *add = Some(board.clone());
                        egui::Popup::close_all(ui.ctx());
                    }
                }
                ui.separator();
                if ui.selectable_label(false, "Cut a panel…").clicked() {
                    *cut = sizes.first().cloned();
                    egui::Popup::close_all(ui.ctx());
                }
            });
        }
    };
    egui::Popup::menu(&response)
        .frame(crate::ui_kit::Container::menu_frame())
        .show(|ui| {
            for (folder, materials) in groups {
                match folder {
                    None => list(ui, materials, &mut add, &mut cut),
                    Some(name) => {
                        crate::ui_kit::Menu::button(ui, name, |ui| list(ui, materials, &mut add, &mut cut));
                    }
                }
            }
        });
    (add, cut)
}

/// The `Skin ▾` button: folder, then material, and the choice skins every selected body in that board's colour and material.
/// Show the grain, or turn it on the boards picked. One menu, so a turn reads as something done *to* the grain.
///
/// A turn only ever touches the selected boards: with nothing picked the item says so and is disabled, because turning the
/// whole drawing by accident is the mistake the first version made on every click. Each selected board swaps on its own,
/// so a top and a side picked together each turn. "All boards along their length" is the one way to change them all.
fn grain_menu(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    let on = GRAIN_ON.load(std::sync::atomic::Ordering::Relaxed);
    if on {
        refresh_grain(app);
    }
    let response = ribbon_button_icon(ui, "grain", rect, "Grain", true, false, Some("material"));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let response = ribbon_button_icon(ui, "grain", rect, "Grain", true, open, Some("material"));
    let chosen = if open { selected_boards(app) } else { Vec::new() };
    let any_turned = GRAIN_CACHE.lock().map(|cache| cache.boards.iter().any(|b| b.across)).unwrap_or(false);
    let mut show: Option<bool> = None;
    let mut turn = false;
    let mut reset = false;
    egui::Popup::menu(&response)
        .frame(crate::ui_kit::Container::menu_frame())
        .show(|ui| {
            let row = ui.selectable_label(on, "Show grain direction");
            crate::scenario::publish_handle("ledger:grain_show", row.rect.center());
            if row.clicked() {
                show = Some(!on);
                egui::Popup::close_all(ui.ctx());
            }
            ui.separator();
            let label = match chosen.len() {
                0 => "Turn grain on selected boards".to_owned(),
                1 => format!("Turn grain on {}", chosen[0]),
                n => format!("Turn grain on {n} selected boards"),
            };
            let row = ui
                .add_enabled(!chosen.is_empty(), egui::Button::new(label).frame(false))
                .on_disabled_hover_text("Select a board first - click it, or Ctrl+click several - then turn its grain.");
            crate::scenario::publish_handle("ledger:grain_flip", row.rect.center());
            if row.clicked() {
                turn = true;
                egui::Popup::close_all(ui.ctx());
            }
            let row = ui
                .add_enabled(any_turned, egui::Button::new("All boards along their length").frame(false))
                .on_hover_text("Put every turned board back to grain along its long side");
            if row.clicked() {
                reset = true;
                egui::Popup::close_all(ui.ctx());
            }
        });
    if let Some(wanted) = show {
        GRAIN_ON.store(wanted, std::sync::atomic::Ordering::Relaxed);
        if wanted {
            clear_grain(app);
            refresh_grain(app);
            let count = GRAIN_CACHE.lock().map(|c| c.boards.len()).unwrap_or(0);
            app.set_status(format!("grain shown on {count} {}", if count == 1 { "board" } else { "boards" }), false);
        } else {
            app.set_status("grain hidden", false);
        }
    }
    if turn || reset {
        let names = if turn { Some(chosen.as_slice()) } else { None };
        match turn_grain(app, names) {
            Ok(count) => {
                // A turn is something to look at: the arrows come on with it.
                GRAIN_ON.store(true, std::sync::atomic::Ordering::Relaxed);
                refresh_grain(app);
                let what = if turn {
                    match chosen.as_slice() {
                        [one] => format!("grain turned on {one}"),
                        _ => format!("grain turned on {count} boards"),
                    }
                } else {
                    format!("{count} {} back to grain along the length", if count == 1 { "board" } else { "boards" })
                };
                app.set_status(format!("{what} - Ctrl+Z undoes it"), false);
            }
            Err(error) => app.set_status(format!("grain not changed: {error}"), true),
        }
    }
}

fn skin_dropdown(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect, groups: &[(Option<String>, Vec<(String, Vec<Board>)>)]) {
    let selected = selected_bodies(app);
    let mut skin: Option<Board> = None;
    let response = ribbon_button_icon(ui, "skin", rect, "Skin", true, false, Some("render_paint"));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let response = ribbon_button_icon(ui, "skin", rect, "Skin", true, open, Some("render_paint"));
    crate::scenario::publish_handle("ledger:skin_menu", rect.center());

    let list = |ui: &mut egui::Ui, materials: &[(String, Vec<Board>)], skin: &mut Option<Board>| {
        for (name, sizes) in materials {
            if let Some(board) = sizes.first()
                && ui.selectable_label(false, name).clicked()
            {
                *skin = Some(board.clone());
                egui::Popup::close_all(ui.ctx());
            }
        }
    };
    egui::Popup::menu(&response)
        .frame(crate::ui_kit::Container::menu_frame())
        .show(|ui| {
            if selected.is_empty() {
                ui.label(
                    egui::RichText::new("select bodies in the model first")
                        .size(Tokens::get().font_small)
                        .color(Tokens::get().text_dim),
                );
            }
            for (folder, materials) in groups {
                match folder {
                    None => list(ui, materials, &mut skin),
                    Some(name) => {
                        crate::ui_kit::Menu::button(ui, name, |ui| list(ui, materials, &mut skin));
                    }
                }
            }
        });
    if let Some(board) = skin {
        // A picked face or edge means its body: that is what a person clicking a board's side meant.
        let chosen = crate::ledger_resize::selected(app);
        if chosen.is_empty() {
            app.set_status("skin: select bodies in the model first", true);
        } else {
            skin_or_resize(app, &board, chosen);
        }
    }
}

/// A skin in the board's own thickness just skins. One in another thickness opens the resize preview first: the board
/// is thinned or thickened on its inside and the boards against it follow, once the operator has seen what will move.
fn skin_or_resize(app: &mut SolveApp, board: &Board, chosen: Vec<String>) {
    let boxes = crate::ledger_resize::boxes(app);
    let off: Vec<String> = chosen
        .iter()
        .filter(|name| {
            boxes.iter().find(|b| &b.name == *name).is_some_and(|b| {
                let mut size = [b.hi[0] - b.lo[0], b.hi[1] - b.lo[1], b.hi[2] - b.lo[2]];
                size.sort_by(f64::total_cmp);
                (size[0] - board.thickness).abs() > SLACK_MM
            })
        })
        .cloned()
        .collect();
    if off.is_empty() {
        skin_bodies(app, board, &chosen);
        return;
    }
    let plan = crate::ledger_resize::plan_all(&boxes, &off, board.thickness);
    let markers = crate::ledger_cams::has_markers(app);
    // The resized design, run through the hole engine before anything moves: what the joints will get, and which get none.
    let after = match &plan {
        Ok(_) => crate::ledger_resize::after_all(&boxes, &off, board.thickness)
            .map(|after| crate::ledger_cams::dry_run(app, &after))
            .unwrap_or_default(),
        Err(_) => (0, 0, Vec::new()),
    };
    // What will move is selected while the preview is up, so the model shows it as well as the list does.
    if let Ok(plan) = &plan {
        let mut lit: Vec<String> = Vec::new();
        for step in &plan.steps {
            let (crate::ledger_resize::Step::Offset { body, .. } | crate::ledger_resize::Step::Shift { body, .. }) = step;
            if !lit.contains(body) {
                lit.push(body.clone());
            }
        }
        app.session.selection = lit.into_iter().map(|name| solidcraft_engine::Sel::Body { name }).collect();
    }
    *PENDING_RESIZE.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(PendingResize { board: board.clone(), bodies: chosen, plan, markers, after, recalc: true });
}

/// A resize waiting for the operator's answer.
struct PendingResize {
    board: Board,
    bodies: Vec<String>,
    plan: Result<crate::ledger_resize::Plan, String>,
    /// What the design already has that a resize leaves out of date: (cams/dowels, hole markers).
    markers: (bool, bool),
    /// What recalculating gives on the resized design: (cams, dowels, joints refused for a 1/2" board).
    after: (usize, usize, Vec<String>),
    /// Recalculate cams and holes after the resize (the operator can untick it).
    recalc: bool,
}

static PENDING_RESIZE: std::sync::Mutex<Option<PendingResize>> = std::sync::Mutex::new(None);

/// The resize preview: what will move, and Resize / Skin only / Cancel.
fn resize_window(app: &mut SolveApp, ctx: &egui::Context) {
    let t = Tokens::get();
    let mut guard = PENDING_RESIZE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(pending) = guard.as_mut() else { return };
    let title = format!("Change to {} {}", sheet_class_label(pending.board.thickness), pending.board.material);
    let ok = if pending.plan.is_ok() { "Resize and skin" } else { "" };
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("sc_ledger_resize"), "render_paint", &title)
        .actions(ok, "Cancel", Some("Skin only"))
        .show(ctx, pending.plan.is_ok(), |ui| match &pending.plan {
            Ok(plan) => {
                ui.label(egui::RichText::new("Nothing has moved yet. This will:").color(t.text_dim));
                for line in &plan.lines {
                    ui.add(egui::Label::new(egui::RichText::new(format!("•  {line}")).color(t.text)).wrap());
                }
                let (cams, dowels, thin) = &pending.after;
                // The owner's rule: no hardware in 1/2" board. Said before the resize, in amber, joint by joint.
                if !thin.is_empty() {
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!(
                                "⚠  No hardware can go in 1/2\" board. After this, {} {} will get no cams or dowels:",
                                thin.len(),
                                if thin.len() == 1 { "joint" } else { "joints" }
                            ))
                            .color(egui::Color32::from_rgb(0xE8, 0xA3, 0x3D)),
                        )
                        .wrap(),
                    );
                    for line in thin {
                        ui.label(egui::RichText::new(format!("     {line}")).color(egui::Color32::from_rgb(0xE8, 0xA3, 0x3D)));
                    }
                }
                if pending.markers.0 || pending.markers.1 {
                    ui.add_space(6.0);
                    let what = if pending.markers.1 { "cams, dowels and holes" } else { "cams and dowels" };
                    ui.checkbox(
                        &mut pending.recalc,
                        format!("Recalculate {what} afterwards ({cams} cams, {dowels} dowels)"),
                    );
                    if !pending.recalc {
                        ui.label(
                            egui::RichText::new("The ones in the design now will be in the wrong places.")
                                .small()
                                .color(egui::Color32::from_rgb(0xE8, 0xA3, 0x3D)),
                        );
                    }
                }
            }
            Err(why) => {
                ui.label(egui::RichText::new(why).color(t.error));
            }
        });
    let answer = match action {
        crate::ui_kit::Action::Ok => Some(true),
        crate::ui_kit::Action::Apply => Some(false),
        crate::ui_kit::Action::Cancel => None,
        _ if closed => None,
        _ => return,
    };
    let pending = guard.take().expect("checked above");
    drop(guard);
    app.session.selection.clear();
    match answer {
        Some(true) => {
            let Ok(plan) = &pending.plan else { return };
            match crate::ledger_resize::apply(app, plan) {
                Ok(()) => {
                    for body in &pending.bodies {
                        retag_thickness(app, body, &pending.board);
                    }
                    skin_bodies(app, &pending.board, &pending.bodies);
                    let moved = plan.steps.len().saturating_sub(pending.bodies.len());
                    // The joints moved with the boards: their fittings and holes are drawn again where they now are.
                    if pending.recalc && (pending.markers.0 || pending.markers.1) {
                        let cams = crate::ledger_cams::calc(app);
                        let message = if pending.markers.1 {
                            crate::ledger_cams::holes(app).message()
                        } else {
                            cams.message()
                        };
                        let warn = cams.is_warning();
                        app.set_status(
                            format!(
                                "{} resized to {}; {moved} {} adjusted. {message}",
                                pending.bodies.join(", "),
                                sheet_class_label(pending.board.thickness),
                                if moved == 1 { "board" } else { "boards" }
                            ),
                            warn,
                        );
                        return;
                    }
                    app.set_status(
                        format!(
                            "{} resized to {} and skinned {}; {moved} {} adjusted to meet {} - run Calc Camlocks again",
                            pending.bodies.join(", "),
                            sheet_class_label(pending.board.thickness),
                            pending.board.material,
                            if moved == 1 { "board" } else { "boards" },
                            if pending.bodies.len() == 1 { "it" } else { "them" }
                        ),
                        false,
                    );
                }
                Err(why) => app.set_status(why, true),
            }
        }
        Some(false) => skin_bodies(app, &pending.board, &pending.bodies),
        None => app.set_status("thickness not changed", false),
    }
}

/// "1/2\"" for 12.7, else the millimetres.
fn sheet_class_label(mm: f64) -> String {
    for (inches, label) in [(12.7, "1/2\""), (19.05, "3/4\""), (25.4, "1\""), (28.575, "1-1/8\"")] {
        if (mm - inches).abs() <= 0.25 {
            return label.to_owned();
        }
    }
    format!("{mm:.1} mm")
}

/// A resized board's stock record follows its new thickness, or the guard would take the resize back as a stretch.
fn retag_thickness(app: &mut SolveApp, name: &str, board: &Board) {
    let mut meta = ledger_meta(app);
    let Some(tag) = meta.get_mut("boards").and_then(|boards| boards.get_mut(name)).and_then(serde_json::Value::as_object_mut) else {
        return;
    };
    tag.insert("thickness".to_owned(), json!(board.thickness));
    tag.insert("class".to_owned(), json!(solidcraft_class(board)));
    tag.insert("material".to_owned(), json!(board.material));
    let _ = app.run("document.metadata.set", json!({ "values": { KEY: meta } }));
}

/// The panel form, in a card of its own: a width and a height on the board that was chosen, its sheet as the default.
fn cut_panel_window(app: &mut SolveApp, ctx: &egui::Context, board: &Board) {
    let t = Tokens::get();
    let id = egui::Id::new("sc_ledger_cut_panel");
    let mut form = ctx.data(|data| data.get_temp::<PanelForm>(form_id())).unwrap_or_else(|| PanelForm::for_board(board));
    form.board = Some(board.material.clone());
    let valid = form.size(board).is_ok();
    let (closed, action) =
        crate::ui_kit::Window::new(id, "sketch", "Cut a panel").actions("Insert", "Cancel", None).fields().show(ctx, valid, |ui| {
            ui.label(egui::RichText::new(format!("from {} · {}", board.material, board_size_label(board))).color(t.text));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut form.width).hint_text("width").desired_width(96.0));
                ui.label("x");
                ui.add(egui::TextEdit::singleline(&mut form.height).hint_text("height").desired_width(96.0));
                ui.label(egui::RichText::new("mm").small().color(t.text_dim));
            });
            if let Err(why) = form.size(board) {
                ui.label(egui::RichText::new(why).small().color(t.error));
            }
        });
    // The kit window answers with what was pressed; `closed` is only its own X. Either way the form is over, so the board it
    // was cutting is forgotten here rather than left to reappear next time the tab is drawn.
    match action {
        crate::ui_kit::Action::Ok => {
            ctx.data_mut(|data| data.remove::<Board>(cut_board_id()));
            ctx.data_mut(|data| data.remove::<PanelForm>(form_id()));
            if let Ok(size) = form.size(board) {
                match insert(app, board, size) {
                    Ok(name) => app.set_status(format!("{name} added"), false),
                    Err(why) => app.set_status(why, true),
                }
            }
        }
        crate::ui_kit::Action::Cancel => {
            ctx.data_mut(|data| data.remove::<Board>(cut_board_id()));
            ctx.data_mut(|data| data.remove::<PanelForm>(form_id()));
        }
        _ if closed => {
            ctx.data_mut(|data| data.remove::<Board>(cut_board_id()));
            ctx.data_mut(|data| data.remove::<PanelForm>(form_id()));
        }
        _ => {
            ctx.data_mut(|data| data.insert_temp(form_id(), form));
        }
    }
}

/// The tab's ribbon: groups of buttons laid out exactly as the other tabs lay out their sections - buttons across the top, the
/// group's name under them, a hairline after it, and every group starting on the same line.
pub fn ribbon(app: &mut SolveApp, ui: &mut egui::Ui, area: Rect) {
    let t = Tokens::get();
    let mut sub = subtab(ui.ctx());
    let top = area.top();
    let mut x = area.left();
    ui.set_clip_rect(area);

    // 1 - what the tab lists: the shop asked for these two stacked, one over the other, in a column of their own.
    let lists = [("Materials", Subtab::Materials), ("Hardware", Subtab::Hardware)];
    let width = lists.iter().map(|(label, _)| button_width(ui, label)).fold(0.0_f32, f32::max) + 2.0 * INSET;
    let mut y = top + 2.0;
    for (label, which) in lists {
        let rect = Rect::from_min_size(pos2(x + INSET, y), vec2(width - 2.0 * INSET, STACKED_HEIGHT));
        if ribbon_button(ui, label, rect, label, false, sub == which).clicked() {
            sub = which;
        }
        crate::scenario::publish_target(&format!("ledger:subtab:{label}"), rect);
        y += STACKED_HEIGHT + BUTTON_GAP;
    }
    set_subtab(ui.ctx(), sub);
    group_caption(ui, area, x, width, "List");
    x += width + PANEL_GAP;

    group_rule(ui, area, x);
    x = match sub {
        Subtab::Materials => materials_panel(app, ui, area, x),
        Subtab::Hardware => hardware_panel(app, ui, area, x),
    };
    group_rule(ui, area, x);

    // The view switch is the tab's, not the materials list's: on the hardware subtab the model still has to be readable, and the
    // shop asked to swap views while working rather than through the host program.
    let wire_w = button_width(ui, "edges");
    let grain_w = button_width(ui, "Grain");
    let width = wire_w + grain_w + BUTTON_GAP + 2.0 * INSET;
    let wire_rect = Rect::from_min_size(pos2(x + INSET, top + 2.0), vec2(wire_w, BUTTON_HEIGHT));
    let mut wireframe = solid_hardware(app);
    if ribbon_button_icon(ui, "wireframe", wire_rect, "edges", false, wireframe, Some("style_wireframe")).clicked() {
        wireframe = !wireframe;
        let style = if wireframe { "wireframe" } else { "shaded_with_edges" };
        let done = app.run("view.visual_style", serde_json::json!({ "style": style })).is_ok();
        WIREFRAME_ON.store(done && wireframe, std::sync::atomic::Ordering::Relaxed);
        if done {
            app.set_status(
                if wireframe {
                    "edges: boards as outlines, hardware solid, so every cam, dowel and hole is in sight"
                } else {
                    "shaded: edges on the faces only"
                },
                false,
            );
        }
    }
    crate::scenario::publish_handle("ledger:wireframe", wire_rect.center());

    // The grain: a menu, the way the rest of the ribbon offers a set of related commands - show it, or turn it round.
    let grain_rect = Rect::from_min_size(pos2(wire_rect.right() + BUTTON_GAP, top + 2.0), vec2(grain_w, BUTTON_HEIGHT));
    grain_menu(app, ui, grain_rect);
    crate::scenario::publish_handle("ledger:grain", grain_rect.center());
    group_caption(ui, area, x, width - 2.0 * INSET, "View");
    x += width + PANEL_GAP;
    group_rule(ui, area, x);

    // Last - the item itself.
    let actions = [
        ("Merge edgebanding", "bands"),
        ("Name parts", "name"),
        ("Picture", "picture"),
        ("Reload", "reload"),
    ];
    let widths = [
        button_width_at(ui, "Merge edgebanding", 118.0),
        button_width(ui, "Name parts"),
        button_width(ui, "Picture"),
        button_width(ui, "Reload"),
    ];
    let width = widths.iter().sum::<f32>() + BUTTON_GAP + 2.0 * INSET;
    let mut bx = x + INSET;
    for ((label, id), button) in actions.iter().zip(widths) {
        let rect = Rect::from_min_size(pos2(bx, top + 2.0), vec2(button, BUTTON_HEIGHT));
        let icon = match *id {
            "picture" => "render_image",
            "name" => "rename",
            "bands" => "edge_continuity",
            _ => "update",
        };
        crate::scenario::publish_handle(&format!("ledger:{id}"), rect.center());
        if ribbon_button_icon(ui, id, rect, label, false, false, Some(icon)).clicked() {
            match *id {
                "bands" => {
                    let report = crate::ledger_edgeband::merge(app);
                    let mut parts: Vec<String> = Vec::new();
                    if report.joined > 0 {
                        parts.push(format!(
                            "{} edgebanding {} merged",
                            report.joined,
                            if report.joined == 1 { "shell" } else { "shells" }
                        ));
                    }
                    if report.edges_dissolved > 0 {
                        parts.push(format!(
                            "dissolved {} seam {}, {} coplanar {}",
                            report.edges_dissolved,
                            if report.edges_dissolved == 1 { "edge" } else { "edges" },
                            report.faces_dissolved,
                            if report.faces_dissolved == 1 { "face" } else { "faces" }
                        ));
                    }
                    if report.removed > 0 {
                        parts.push(format!(
                            "{} hollow {} removed (shell geometry cannot be joined)",
                            report.removed,
                            if report.removed == 1 { "band" } else { "bands" }
                        ));
                    }
                    if report.left > 0 {
                        parts.push(format!("{} left alone", report.left));
                    }
                    app.set_status(
                        if parts.is_empty() {
                            "no edgebanding to merge".to_owned()
                        } else {
                            parts.join(", ")
                        },
                        false,
                    );
                }
                "name" => {
                    let (named, unnamed) = crate::ledger_naming::name_parts(app);
                    app.set_status(
                        match (named, unnamed) {
                            (0, 0) => "nothing in the design to name".to_owned(),
                            (0, _) => format!("no body could be named - {unnamed} left as numbers"),
                            (named, 0) => format!("{named} bodies named"),
                            (named, unnamed) => {
                                format!("{named} bodies named, {unnamed} left as numbers")
                            }
                        },
                        false,
                    );
                }
                "picture" => match picture(app) {
                    Ok(note) => app.set_status(note, false),
                    Err(why) => app.set_status(why, true),
                },
                _ => {
                    boards(true);
                    hardware(true);
                    app.set_status("Ledger's boards and hardware read again", false);
                }
            }
        }
        crate::scenario::publish_target(&format!("ledger:{}", if *id == "picture" { "picture" } else { "reload" }), rect);
        bx += button + BUTTON_GAP;
    }
    group_caption(ui, area, x, width, "Item");
    let _ = t;

    // The panel form is the tab's, not the menu's: it stays until it is answered, whichever subtab is showing.
    if let Some(board) = ui.ctx().data(|data| data.get_temp::<Board>(cut_board_id())) {
        cut_panel_window(app, ui.ctx(), &board);
    }
    resize_window(app, ui.ctx());
}

/// The boards group: `Board ▾` adds a sheet or opens the panel form, and `snap` closes the gap a moved board leaves.
fn materials_panel(app: &mut SolveApp, ui: &mut egui::Ui, area: Rect, mut x: f32) -> f32 {
    let t = Tokens::get();
    let top = area.top();
    let (shop_boards, error) = boards(false);
    if let Some(error) = &error {
        ui.painter().text(
            pos2(x + INSET, top + BUTTON_HEIGHT / 2.0),
            Align2::LEFT_CENTER,
            format!("Ledger's boards could not be read - {error}"),
            crate::fonts::semibold(t.font_small),
            t.text_dim,
        );
        crate::scenario::publish_handle("ledger:boards_error", pos2(x + INSET, top + BUTTON_HEIGHT / 2.0));
        return x;
    }
    if shop_boards.is_empty() {
        ui.painter().text(
            pos2(x + INSET, top + BUTTON_HEIGHT / 2.0),
            Align2::LEFT_CENTER,
            "Ledger has no boards yet - add a material there, with its sheet size",
            crate::fonts::semibold(t.font_small),
            t.text_dim,
        );
        return x;
    }
    let groups = grouped(&shop_boards);

    // Boards: add a sheet, and the snap switch that belongs with moving one.
    let board_w = SQUARE;
    let snap_w = SQUARE;
    let width = board_w + snap_w + BUTTON_GAP + 2.0 * INSET;
    let board_rect = Rect::from_min_size(pos2(x + INSET, top + 2.0), vec2(board_w, BUTTON_HEIGHT));
    let (add, cut) = board_dropdown(ui, board_rect, &groups);
    let mut snap = SNAP_ON.load(std::sync::atomic::Ordering::Relaxed);
    let snap_rect = Rect::from_min_size(pos2(board_rect.right() + BUTTON_GAP, top + 2.0), vec2(snap_w, BUTTON_HEIGHT));
    if ribbon_button_icon(ui, "snap", snap_rect, "snap", false, snap, Some("align")).clicked() {
        snap = !snap;
        SNAP_ON.store(snap, std::sync::atomic::Ordering::Relaxed);
    }
    crate::scenario::publish_target("ledger:snap", snap_rect);
    group_caption(ui, area, x, width, "Boards");
    x += width + PANEL_GAP;

    // Skin: the material the selected bodies are faced in.
    let skin_w = SQUARE;
    let width = skin_w + 2.0 * INSET;
    let skin_rect = Rect::from_min_size(pos2(x + INSET, top + 2.0), vec2(skin_w, BUTTON_HEIGHT));
    skin_dropdown(app, ui, skin_rect, &groups);
    group_caption(ui, area, x, width, "Skin");
    x += width + PANEL_GAP;

    if let Some(board) = add {
        let sheet = (board.width.max(board.height), board.width.min(board.height));
        match insert(app, &board, sheet) {
            Ok(name) => app.set_status(format!("{name} added"), false),
            Err(why) => app.set_status(why, true),
        }
    }
    if let Some(board) = cut {
        ui.ctx().data_mut(|data| data.insert_temp(cut_board_id(), board));
    }
    x
}

/// The Hardware subtab: the actions that place and drill hardware. The read-only list of Ledger's hardware that stood
/// first here is gone - the room is for "add a hinge"-style buttons, one per kind of fitting.
fn hardware_panel(app: &mut SolveApp, ui: &egui::Ui, area: Rect, x: f32) -> f32 {
    let top = area.top();
    let mut x = x;
    // Counting camlocks happens here now: on the design, where the seams can be seen while they are worked out.
    let label = "Calc Camlocks";
    let width = button_width_at(ui, label, 110.0) + 2.0 * INSET;
    let rect = Rect::from_min_size(pos2(x + INSET, top + 2.0), vec2(width - 2.0 * INSET, BUTTON_HEIGHT));
    if ribbon_button_icon(ui, "camlocks", rect, label, false, false, Some("edge_continuity")).clicked() {
        // Hardware moved or flipped by hand is the operator's: ask before the rule puts it back.
        let hand = crate::ledger_hand::hand_changed(app);
        if hand.is_empty() {
            let outcome = crate::ledger_cams::calc(app);
            app.set_status(outcome.message(), outcome.is_warning());
        } else {
            *PENDING_KEEP.lock().unwrap_or_else(|e| e.into_inner()) = Some(hand);
        }
    }
    crate::scenario::publish_target("ledger:camlocks", rect);
    // The bores themselves, cut into the boards: what the shop floor's machine will drill.
    let holes_label = "Calc Holes";
    let holes_width = button_width_at(ui, holes_label, 90.0) + 2.0 * INSET;
    let holes_rect = Rect::from_min_size(pos2(x + width, top + 2.0), vec2(holes_width - 2.0 * INSET, BUTTON_HEIGHT));
    if ribbon_button_icon(ui, "holes", holes_rect, holes_label, false, false, Some("hole")).clicked() {
        let outcome = crate::ledger_cams::holes(app);
        app.set_status(outcome.message(), !outcome.too_thin.is_empty() || !outcome.problems.is_empty());
    }
    crate::scenario::publish_target("ledger:holes", holes_rect);
    let mut width = width + holes_width;
    // Turn a cam over so it sits in the other face of its board; Calc Holes then drills it where it now is.
    let flip_label = "Flip cam";
    let flip_width = button_width_at(ui, flip_label, 84.0) + 2.0 * INSET;
    let flip_rect = Rect::from_min_size(pos2(x + width, top + 2.0), vec2(flip_width - 2.0 * INSET, BUTTON_HEIGHT));
    if ribbon_button_icon(ui, "flip_cam", flip_rect, flip_label, false, false, Some("mirror")).clicked() {
        let chosen: Vec<String> =
            crate::ledger_resize::selected(app).into_iter().filter(|name| name.starts_with("cam ")).collect();
        if chosen.is_empty() {
            app.set_status("flip cam: select the cams to flip in the model first", true);
        } else {
            let flipped = crate::ledger_hand::as_one_step(app, "Flip cam", |app| crate::ledger_hand::flip(app, &chosen));
            let mut line = if flipped.done.is_empty() {
                "nothing flipped".to_owned()
            } else {
                format!("flipped {} to the other face of {} board - press Calc Holes to drill for it", flipped.done.join(", "), if flipped.done.len() == 1 { "its" } else { "their" })
            };
            if !flipped.visible.is_empty() {
                line.push_str(&format!("; check: {} now open on a face the finished piece shows", flipped.visible.join(", ")));
            }
            if !flipped.refused.is_empty() {
                line.push_str(&format!("; {} could not be flipped (press Calc Camlocks first)", flipped.refused.join(", ")));
            }
            app.set_status(line, flipped.done.is_empty() || !flipped.visible.is_empty());
        }
    }
    crate::scenario::publish_target("ledger:flip_cam", flip_rect);
    width += flip_width;
    let clear_label = "Clear holes";
    let clear_width = button_width_at(ui, clear_label, 84.0) + 2.0 * INSET;
    let clear_rect = Rect::from_min_size(pos2(x + width, top + 2.0), vec2(clear_width - 2.0 * INSET, BUTTON_HEIGHT));
    if ribbon_button_icon(ui, "clear_holes", clear_rect, clear_label, false, false, Some("delete")).clicked() {
        let n = crate::ledger_cams::clear_holes(app);
        app.set_status(format!("{n} hole markers and the machine hole list cleared; the hardware is untouched"), false);
    }
    crate::scenario::publish_target("ledger:clear_holes", clear_rect);
    width += clear_width;
    // Hardware by hand: a cam, a wooden dowel or a foot screw on the boards that are selected.
    let add_label = "Add";
    let add_width = button_width_at(ui, add_label, 64.0) + 2.0 * INSET;
    let add_rect = Rect::from_min_size(pos2(x + width, top + 2.0), vec2(add_width - 2.0 * INSET, BUTTON_HEIGHT));
    add_dropdown(app, ui, add_rect);
    crate::scenario::publish_target("ledger:add_hardware", add_rect);
    width += add_width;
    // The same snap switch as beside the boards: here it keeps moved hardware on its joint.
    let snap_rect = Rect::from_min_size(pos2(x + width, top + 2.0), vec2(SQUARE, BUTTON_HEIGHT));
    let mut snap = SNAP_ON.load(std::sync::atomic::Ordering::Relaxed);
    if ribbon_button_icon(ui, "hw_snap", snap_rect, "snap", false, snap, Some("align")).clicked() {
        snap = !snap;
        SNAP_ON.store(snap, std::sync::atomic::Ordering::Relaxed);
        app.set_status(if snap { "snap on: moved hardware slides along its joint" } else { "snap off: hardware moves freely" }, false);
    }
    crate::scenario::publish_target("ledger:hw_snap", snap_rect);
    width += SQUARE + INSET;
    group_caption(ui, area, x, width, "Camlocks & holes");
    x += width + PANEL_GAP;
    keep_window(app, ui.ctx());
    x
}

/// The `Add ▾` button: pick a cam, a wooden dowel or a foot screw, and it is put onto the selected boards
/// ([`crate::ledger_insert`]); the status line says what was added or why nothing was.
fn add_dropdown(app: &mut SolveApp, ui: &egui::Ui, rect: Rect) {
    let response = ribbon_button_icon(ui, "add_hardware", rect, "Add", true, false, Some("plus"));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let response = ribbon_button_icon(ui, "add_hardware", rect, "Add", true, open, Some("plus"));
    let mut chosen = None;
    egui::Popup::menu(&response)
        .frame(crate::ui_kit::Container::menu_frame())
        .show(|ui| {
            ui.label(
                egui::RichText::new("select the two boards it joins (a foot screw: the board above the foot; a handle: the board it goes on)")
                    .size(Tokens::get().font_small)
                    .color(Tokens::get().text_dim),
            );
            for what in crate::ledger_insert::Add::ALL {
                let item = ui.selectable_label(false, what.label());
                crate::scenario::publish_handle(&format!("ledger:add:{}", what.label().to_lowercase()), item.rect.center());
                if item.clicked() {
                    chosen = Some(what);
                    egui::Popup::close_all(ui.ctx());
                }
            }
        });
    if let Some(what) = chosen {
        match crate::ledger_insert::add(app, what) {
            Ok(line) => app.set_status(line, false),
            Err(why) => app.set_status(why, true),
        }
    }
}

/// Hand-changed fittings waiting for the operator's answer to a second Calc Camlocks.
static PENDING_KEEP: std::sync::Mutex<Option<std::collections::BTreeSet<String>>> = std::sync::Mutex::new(None);

/// "Keep my changes" / "Reset all to the rule" / Cancel.
fn keep_window(app: &mut SolveApp, ctx: &egui::Context) {
    let t = Tokens::get();
    let mut guard = PENDING_KEEP.lock().unwrap_or_else(|e| e.into_inner());
    let Some(hand) = guard.as_ref() else { return };
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("sc_ledger_keep"), "edge_continuity", "Calc Camlocks")
        .actions("Keep my changes", "Cancel", Some("Reset all to the rule"))
        .show(ctx, true, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "{} {} added, moved, flipped or deleted by hand: {}.",
                        hand.len(),
                        if hand.len() == 1 { "fitting was" } else { "fittings were" },
                        hand.iter().cloned().collect::<Vec<_>>().join(", ")
                    ))
                    .color(t.text),
                )
                .wrap(),
            );
            ui.label(
                egui::RichText::new("Keep them where they are and place only the rest by the rule, or put everything back by the rule?")
                    .color(t.text_dim),
            );
        });
    let keep = match action {
        crate::ui_kit::Action::Ok => Some(true),
        crate::ui_kit::Action::Apply => Some(false),
        crate::ui_kit::Action::Cancel => None,
        _ if closed => None,
        _ => return,
    };
    let hand = guard.take().expect("checked above");
    drop(guard);
    match keep {
        Some(true) => {
            let outcome = crate::ledger_cams::calc_keep(app, &hand);
            app.set_status(format!("kept {} by hand; {}", hand.len(), outcome.message()), outcome.is_warning());
        }
        Some(false) => {
            let outcome = crate::ledger_cams::calc(app);
            app.set_status(format!("reset to the rule; {}", outcome.message()), outcome.is_warning());
        }
        None => app.set_status("Calc Camlocks cancelled - nothing changed", false),
    }
}

/// The bodies selected in the model, by name: bodies only, because a face or an edge is not something a material covers.
fn selected_bodies(app: &SolveApp) -> Vec<String> {
    app.session
        .selection
        .iter()
        .filter_map(|sel| match sel {
            solidcraft_engine::Sel::Body { name } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// Skin each body in one of the shop's boards: the material the audit reads, and the colour the model is drawn in.
///
/// SolidCraft assigns a physical material by name, and only one the design already defines. A design opened from Ledger's
/// folder carries the materials that existed when it was saved, so a board the shop added later is missing from it: the
/// material is created first, exactly as `insert` does. A board with no density in Ledger cannot be one - the shop does not
/// guess weights - so it is drawn in its colour and the status line says what is missing, instead of an error toast.
fn skin_bodies(app: &mut SolveApp, board: &Board, bodies: &[String]) {
    let plural = if bodies.len() == 1 { "y" } else { "ies" };
    let Some(density) = board.density_g_cm3 else {
        let _ = app.run("appearance.assign", json!({ "bodies": bodies, "color": board.colour }));
        app.set_status(
            format!(
                "{} bod{plural} coloured {} - no physical material yet: give {} a density in Ledger (Hardware & Materials) so it counts in weights",
                bodies.len(),
                board.material,
                board.material
            ),
            false,
        );
        return;
    };
    match ensure_material(app, &board.material, density, &board.colour) {
        Ok(()) => {}
        Err(error) => {
            app.set_status(format!("{} could not be added to this design: {error}", board.material), true);
            return;
        }
    }
    if app.run("material.assign", json!({ "bodies": bodies, "material": board.material })).is_ok() {
        let _ = app.run("appearance.assign", json!({ "bodies": bodies, "color": board.colour }));
        app.set_status(format!("{} bod{plural} skinned in {}", bodies.len(), board.material), false);
    }
}

/// Define a physical material in the open design unless it already is.
fn ensure_material(app: &mut SolveApp, name: &str, density: f64, colour: &str) -> Result<(), String> {
    let listed = app.run("material.list", json!({}))?;
    if !listed_has(&listed, name) {
        app.run("material.create", json!({ "name": name, "density_g_cm3": density, "color": colour }))?;
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The SOL-DKS-2448 desk as measured: the top, the modesty and the two legs are boards; a cam block and a glide are not.
    #[test]
    fn the_grain_goes_on_boards_and_follows_the_turned_list() {
        let measured = serde_json::json!({"bodies": [
            {"name": "Part (2)", "bbox": {"min": [0.0, -600.07, 736.6], "max": [1200.15, 0.0, 762.0]}},
            {"name": "Part (9)", "bbox": {"min": [20.6, -590.6, 7.0], "max": [1179.1, -571.55, 736.6]}},
            {"name": "Part (10)", "bbox": {"min": [1179.1, -600.1, 7.0], "max": [1198.15, -0.02, 736.6]}},
            {"name": "cam 1", "bbox": {"min": [13.1, -588.0, 243.1], "max": [28.1, -573.0, 257.3]}},
            {"name": "Part (3)", "bbox": {"min": [0.4, -458.3, -1.8], "max": [21.7, -437.0, 8.7]}},
            {"name": "grain 1", "bbox": {"min": [0.0, 0.0, 0.0], "max": [100.0, 100.0, 1.0]}}
        ]});
        let across: std::collections::BTreeSet<String> = ["Part (9)".to_owned()].into();
        let boards = grain_boards(&measured, &across);
        let names: Vec<&str> = boards.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Part (2)", "Part (9)", "Part (10)"]);
        // The top runs along its 1200 mm (x); the modesty is turned, so it runs up its 729.6 mm (z), not along its 1158 mm.
        assert_eq!(boards[0].run_axis(), 0);
        assert_eq!(boards[1].run_axis(), 2);
        // The leg runs up its height: 729.6 (z) is longer than its 600 (y).
        assert_eq!(boards[2].run_axis(), 2);
    }

    /// The arrow goes on the face turned towards the eye - the near side of the modesty from the front, the far side from
    /// behind - and on neither when the board is seen edge-on.
    #[test]
    fn the_arrow_sits_on_the_face_the_eye_sees() {
        let modesty = GrainBoard { name: "M".into(), lo: [0.0, -590.6, 7.0], hi: [1158.0, -571.55, 736.6], across: false };
        let (front, normal) = facing_side(&modesty, [0.2, 0.9, -0.3]).expect("seen from the front");
        assert!(front[1] < -590.6 && normal[1] < 0.0, "{front:?}");
        let (back, normal) = facing_side(&modesty, [0.2, -0.9, -0.3]).expect("seen from behind");
        assert!(back[1] > -571.55 && normal[1] > 0.0, "{back:?}");
        assert!(facing_side(&modesty, [1.0, 0.0, 0.0]).is_none(), "edge-on");
    }

    /// A body between the eye and a board hides the board's arrow; one beside the line of sight does not.
    #[test]
    fn a_ray_meets_the_boxes_in_its_way() {
        let (origin, dir) = ([0.0, -2000.0, 0.0], [0.0, 1.0, 0.0]);
        assert_eq!(ray_hits_box(origin, dir, [-10.0, -100.0, -10.0], [10.0, 0.0, 10.0]), Some(1900.0));
        assert_eq!(ray_hits_box(origin, dir, [50.0, -100.0, -10.0], [70.0, 0.0, 10.0]), None);
        assert_eq!(ray_hits_box(origin, [0.0, -1.0, 0.0], [-10.0, -100.0, -10.0], [10.0, 0.0, 10.0]), None, "behind the eye");
    }

    /// The turned list is read from the design's metadata, and a design without one turns nothing.
    #[test]
    fn the_turned_list_is_read_from_the_design() {
        let meta = serde_json::json!({"boards": {}, "grain_across": ["Part (9)", "Part (10)"]});
        // A rename carries the turn and the sheet limits with it, and two bodies trading names trade their records too.
        let records = serde_json::json!({"boards": {"Part (10)": {"material": "Asian Night"}, "side": {"material": "Classic White"}},
            "grain_across": ["Part (9)", "side"]});
        let renamed = vec![("Part (10)".to_owned(), "side".to_owned()), ("side".to_owned(), "side (2)".to_owned()),
            ("Part (9)".to_owned(), "panel - front".to_owned())];
        let moved = renamed_records(records.as_object().cloned().unwrap(), &renamed);
        assert_eq!(moved["boards"]["side"]["material"], "Asian Night");
        assert_eq!(moved["boards"]["side (2)"]["material"], "Classic White");
        assert_eq!(moved["grain_across"], serde_json::json!(["panel - front", "side (2)"]));
        let records = serde_json::json!({"bores": {"holes": [{"board": "Part (10)", "x": 1.0}, {"board": "top", "x": 2.0}]}});
        let moved = renamed_records(records.as_object().cloned().unwrap(), &renamed);
        assert_eq!(moved["bores"]["holes"][0]["board"], "side");
        assert_eq!(moved["bores"]["holes"][1]["board"], "top");
        let turned = turned_in(meta.as_object().expect("an object"));
        assert!(turned.contains("Part (9)") && turned.contains("Part (10)") && turned.len() == 2);
        assert!(turned_in(&serde_json::Map::new()).is_empty());
    }

    /// `/2` adds the folder a board is filed in; `/1` still reads, so an older Ledger keeps working.
    #[test]
    fn a_board_reads_its_folder_and_the_older_format_still_works() {
        let with_folder = r##"{"format": "ledger-boards/2", "boards": [
            {"material": "Asian Night", "colour": "#424242", "width": 2743, "height": 1829, "thickness": 18,
             "density_g_cm3": 0.7, "folder": "White oak"}
        ]}"##;
        let board = &parse(with_folder).expect("the file reads")[0];
        assert_eq!(board.folder.as_deref(), Some("White oak"));

        let old = r##"{"format": "ledger-boards/1", "boards": [
            {"material": "Asian Night", "colour": "#424242", "width": 2743, "height": 1829, "thickness": 18,
             "density_g_cm3": 0.7}
        ]}"##;
        let board = &parse(old).expect("an older file reads")[0];
        assert_eq!(board.folder, None, "unfiled, not invented");
    }

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
        assert_eq!((board.width, board.height, board.thickness), (2743.0, 1829.0, 18.0));
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

    fn asian_night() -> Board {
        Board {
            material: "Asian Night".to_owned(),
            colour: "#424242".to_owned(),
            width: 2743.0,
            height: 1829.0,
            thickness: 18.0,
            density_g_cm3: Some(0.7),
            folder: None,
        }
    }

    /// A panel may grow back towards its sheet, because a saw can cut a bigger part from a bigger board.
    #[test]
    fn a_cut_panel_may_grow_back_to_its_sheet() {
        let tag = json!({"width": 720.0, "height": 450.0, "max_width": 2743.0, "max_height": 1829.0, "thickness": 18.0});
        assert!(complaint(&tag, [18.0, 450.0, 1000.0]).is_none(), "1000 fits the sheet");
        assert!(complaint(&tag, [18.0, 450.0, 3000.0]).is_some(), "3000 does not");
    }

    /// A tag written before the caps existed still guards the board.
    #[test]
    fn a_tag_without_caps_still_works() {
        let tag = json!({"width": 2743.0, "height": 1829.0, "thickness": 18.0});
        assert!(complaint(&tag, [18.0, 1829.0, 2743.0]).is_none());
        assert!(complaint(&tag, [18.0, 1829.0, 2800.0]).is_some());
    }

    /// The thickness is the one dimension that never moves.
    #[test]
    fn a_thickness_off_its_class_is_a_complaint() {
        let tag = json!({"width": 2743.0, "height": 1829.0, "max_width": 2743.0, "max_height": 1829.0,
                         "thickness": 19.05, "class": "3/4\""});
        assert!(complaint(&tag, [19.05, 900.0, 1800.0]).is_none());
        let why = complaint(&tag, [36.0, 900.0, 1800.0]).expect("36 mm is not 3/4 in");
        assert!(why.contains("3/4"), "{why}");
    }

    /// A panel is only as big as its board: the saw cannot cut bigger than the sheet.
    #[test]
    fn a_panel_is_capped_by_its_sheet() {
        let board = asian_night();
        assert_eq!(cut_size(&board, 720.0, 450.0), Some((720.0, 450.0)));
        assert_eq!(cut_size(&board, 2743.0, 1829.0), Some((2743.0, 1829.0)), "a whole sheet is a panel too");
        assert_eq!(cut_size(&board, 3000.0, 450.0), None, "longer than the sheet");
        assert_eq!(cut_size(&board, 720.0, 2000.0), None, "wider than the sheet");
        assert_eq!(cut_size(&board, 0.0, 450.0), None, "no width");
    }

    /// A panel is named by its size; a whole sheet keeps the board's own name.
    #[test]
    fn a_cut_panel_says_its_size() {
        let board = asian_night();
        assert_eq!(panel_label(&board, (2743.0, 1829.0)), "Asian Night sheet");
        assert_eq!(panel_label(&board, (1829.0, 2743.0)), "Asian Night sheet", "either way round");
        assert_eq!(panel_label(&board, (720.0, 450.0)), "Asian Night 720x450");
    }

    /// A board slid near a face lands flush on it, along that axis and no other.
    #[test]
    fn a_board_slid_near_a_face_lands_flush_on_it() {
        let moving = ([0.0, 0.0, 0.0], [450.0, 720.0, 18.0]);
        let target = ([100.0, -200.0, 24.0], [550.0, 520.0, 42.0]);
        assert_eq!(snap_translation(moving, target, 10.0), Some([0.0, 0.0, 6.0]));
    }

    /// A board nowhere near another is left exactly where it was put.
    #[test]
    fn a_board_far_from_anything_does_not_move() {
        let moving = ([0.0, 0.0, 0.0], [450.0, 720.0, 18.0]);
        let target = ([2000.0, 0.0, 0.0], [2450.0, 720.0, 18.0]);
        assert_eq!(snap_translation(moving, target, 10.0), None);
    }

    /// Boards already sharing space are not pulled apart or pushed together.
    #[test]
    fn a_board_inside_another_is_left_alone() {
        let moving = ([0.0, 0.0, 0.0], [450.0, 720.0, 18.0]);
        let target = ([100.0, 100.0, 0.0], [550.0, 820.0, 18.0]);
        assert_eq!(snap_translation(moving, target, 10.0), None);
    }

    /// A corner touching a face is not a board resting on it.
    #[test]
    fn a_corner_near_a_face_is_not_snapped() {
        let moving = ([0.0, 0.0, 0.0], [450.0, 720.0, 18.0]);
        // Only 20 mm of the 450 mm face overlaps in x, so there is nothing to rest on.
        let target = ([430.0, 0.0, 24.0], [880.0, 720.0, 42.0]);
        assert_eq!(snap_translation(moving, target, 10.0), None);
    }

    /// The hardware list reads the shop's file, and a row with no maker's model still reads.
    #[test]
    fn the_hardware_list_reads_the_shops_file() {
        let text = r##"{"format": "ledger-hardware/1", "hardware": [
            {"kind": "hinge", "name": "Blum 71B3550", "model": "71B3550", "price": 4.1},
            {"kind": "handle", "name": "Bar 128 mm", "model": "", "price": 7.5}
        ]}"##;
        let rows = parse_hardware(text).expect("the file reads");
        assert_eq!(rows.len(), 2);
        assert!(parse_hardware(r#"{"hardware": []}"#).is_err(), "another file is refused");
    }

    /// A board with no size cannot become a body, so it is left out rather than made wrong.
    #[test]
    fn a_board_with_no_size_is_skipped() {
        let text = r##"{"format": "ledger-boards/1", "boards": [
            {"material": "Nowhere", "colour": "#ffffff", "width": 0, "height": 100, "thickness": 18}
        ]}"##;
        assert!(parse(text).expect("the file reads").is_empty());
    }

    fn board(material: &str, folder: Option<&str>, width: f64) -> Board {
        Board {
            material: material.to_owned(),
            colour: "#424242".to_owned(),
            width,
            height: (width * 2.0 / 3.0).round(),
            thickness: 19.05,
            density_g_cm3: Some(0.7),
            folder: folder.map(str::to_owned),
        }
    }

    /// The menu shows what nobody filed first, then one group per folder, and each material's sizes longest first.
    #[test]
    fn the_menu_lists_unfiled_boards_first_then_folders_then_sizes() {
        let boards = vec![
            board("Blonde Surfside", Some("White oak"), 1219.0),
            board("Asian Night", Some("White oak"), 2743.0),
            board("Asian Night", Some("White oak"), 1219.0),
            board("Maple", None, 2743.0),
        ];
        let groups = grouped(&boards);
        assert_eq!(groups.len(), 2, "one group of unfiled boards, one folder");
        assert_eq!(groups[0].0, None);
        assert_eq!(groups[0].1[0].0, "Maple", "unfiled boards come first, as themselves");
        assert_eq!(groups[1].0.as_deref(), Some("White oak"));
        // Materials keep the order Ledger published them in, which is the order the shop sees in its own library.
        let materials: Vec<&str> = groups[1].1.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(materials, vec!["Blonde Surfside", "Asian Night"]);
        let asian = groups[1].1.iter().find(|(name, _)| name == "Asian Night").expect("the board is listed");
        let sizes: Vec<f64> = asian.1.iter().map(|b| b.width).collect();
        assert_eq!(sizes, vec![2743.0, 1219.0], "longest side first");
    }

    /// A size row says the size and the class the shop uses, so two 3/4" boards cannot be confused.
    #[test]
    fn a_size_row_reads_as_a_size_and_a_class() {
        let label = board_size_label(&board("Asian Night", None, 2743.0));
        assert!(label.starts_with("2743 x 1829 · 3/4\""), "{label}");
        assert_eq!(label, "2743 x 1829 · 3/4\" · 66.9 kg", "the shop's own weight, when it has one");
        let mut unweighed = board("Mystery", None, 2743.0);
        unweighed.density_g_cm3 = None;
        assert_eq!(board_size_label(&unweighed), "2743 x 1829 · 3/4\"", "no density, no weight invented");
    }
}
