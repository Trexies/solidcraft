//! Calc Camlocks: the vendor's hole engine, run on the bodies the editor has open, and a block drawn at every
//! **station**.
//!
//! This is the pass the shop runs from Ledger's parts window, moved to where the design is — and it is now literally the
//! same code: `ledger-holes`, the crate Ledger's `src/vendor/` re-exports, which is the port of the vendor client's
//! `CreateHolesInfo` / `AddSanHeYiToPart` / `CheckPunchConflict` chain. The bodies' names and world boxes go in as
//! `translate::Board`s, the engine answers with the holes it would drill, and each station becomes a `cam` block at its
//! mouth. No seam rule of our own is applied here; the count is the engine's.
//!
//! A bore is a block - orange, the size of the cutter - never a hole in the panel, because cutting would destroy the
//! drawing. The block is 15 x 15 x 14.2, the cam cup's own cutter, and it stands at the bore's mouth in model space: the
//! engine reports a hole in its panel's frame, and [`ledger_holes::hole_at_world`] undoes that frame (and the vendor's
//! `0..thickness` z scale) so the block lands where the panel is.
//!
//! # One marker per station, not per bore
//!
//! The engine emits **bores**, and a fastener is several of them. One cam is three: the ⌀8 bolt up the source's edge,
//! the ⌀15 cup 33 mm in on the mating face, and the ⌀10 pre-hole in the mate — which lands on the bolt's *own* world
//! point. One dowel is two, again coincident. Drawing a block per bore therefore drew every joint twice, and at a
//! leg/top corner, where three joints converge (leg→top, apron→top, leg→back panel), a dozen blocks landed in a 150 mm
//! cube. The arithmetic says the fault was never in the holes: all 60 coincidences in the 48x72 fixture are bolt/mate
//! pairs, and no two joints drill the same point.
//!
//! [`ledger_holes::Hole::station`] is the engine's own answer to "which fastener is this bore part of", so the block is
//! placed once per station — 60 of them on the 48x72's 166 bores — and on the bore that *is* the joint: the mate's
//! ⌀10 pre-hole, where the two boards meet ([`ledger_holes::station_anchor`]). Placing it on the cup instead marked only
//! the board that carries the fitting, so the tabletop — 22 pre-holes and 6 dowel mates on part 1, and no cup anywhere —
//! showed no marks at all, and the ones beside it belonged to the rails and legs underneath. Nothing the engine returned
//! changes; the status line states the bores, the fittings and the stations so the three numbers can never be mistaken
//! for each other.
//!
//! # The tree: one `Hardware` folder, one folder per kind
//!
//! What the pass draws is hardware, so it is filed as hardware: every marker lands in a browser folder `Hardware`, and
//! inside it a folder per kind — `Cams`, `Dowels`, `Holes` (a shelf pin's bore is a plain hole). The tree then reads
//! `Bodies › Hardware › Cams`, and the fifty-odd `cam N` rows are one row that folds away, instead of a flat list the
//! panel has to be scrolled through. The folders are design objects (`browser.group`), so they save with the design and
//! undo like any other change. A pass starts by clearing what an earlier pass left — the `cam`/`dowel`/`pin` bodies
//! *and* their folders, wherever in the tree they now sit — so pressing the button twice redraws one set, never two,
//! and never leaves a stale `Hardware` behind.
//!
//! # Which rule set drills the drawing
//!
//! The count comes from `ledger-holes`, and that crate has two rule sets: the vendor's count tables, which drill a
//! 1153 mm seam with four cams, and the shop's **SOL-DKS desk standard**, which drills the desk with eleven because the
//! BOM says eleven. Which one runs is the *item's* answer, not the drawing's: Ledger writes the item it opened into
//! `current.json` beside `boards.json`, and an item whose `construction` is `desk` gets the desk standard. Everything
//! else — a cabinet, a table, an island, or no file at all — keeps the vendor's tables, which is what this button did
//! before. The status line names the rule set, so a count that changed can be traced to the decision rather than
//! looking like the button miscounting.

use std::collections::BTreeSet;

use ledger_holes::construction::{declare_desk_links, desk_declarations, Construction};
use ledger_holes::translate::{translate_boards, Board, Role};
use ledger_holes::{
    hole_at_world, plan, plan_with_standard, station_anchor, Declared, DeskPart, Fastener, Hole, Panel, Plan, Settings,
    Standard,
};
use serde_json::{Value, json};

/// The cam cup's cutter: the block this button has always drawn, wherever it now stands.
const MARKER: f64 = 15.0;
const MARKER_DEEP: f64 = 14.2;
/// The tree folder every marker is filed under; each kind gets its own folder inside it.
const HARDWARE: &str = "Hardware";

struct Box3 {
    name: String,
    lo: [f64; 3],
    hi: [f64; 3],
}

impl Box3 {
    /// A bore marker this button put in the design on an earlier click.
    fn is_marker(&self) -> bool {
        is_marker_name(&self.name)
    }
}

/// Whether a body name is one this pass writes: `cam 7`, `dowel 2`, `pin 1`.
fn is_marker_name(name: &str) -> bool {
    name.starts_with("cam ") || name.starts_with("dowel ") || name.starts_with("pin ")
}

/// The folder a kind of fastener is filed in, and the word its markers are numbered from.
fn family(fastener: Fastener) -> (&'static str, &'static str) {
    match fastener {
        Fastener::Cam => ("Cams", "cam"),
        Fastener::Dowel => ("Dowels", "dowel"),
        Fastener::ShelfPin => ("Holes", "pin"),
    }
}

fn boxes(value: &Value) -> Vec<Box3> {
    let mut out = Vec::new();
    let Some(list) = value.get("bodies").and_then(Value::as_array) else {
        return out;
    };
    for body in list {
        let (Some(name), Some(bbox)) = (body.get("name").and_then(Value::as_str), body.get("bbox")) else {
            continue;
        };
        let corner = |key: &str| -> Option<[f64; 3]> {
            let items = bbox.get(key)?.as_array()?;
            Some([items.first()?.as_f64()?, items.get(1)?.as_f64()?, items.get(2)?.as_f64()?])
        };
        let (Some(lo), Some(hi)) = (corner("min"), corner("max")) else { continue };
        out.push(Box3 { name: name.to_owned(), lo, hi });
    }
    out
}

/// One fastener in the drawing: an engine station, where its block goes, and which fitting it is.
struct Station {
    /// The engine's station id, so two stations can never be the same fastener.
    id: u32,
    fastener: Fastener,
    /// Where the block stands, in model millimetres.
    at: [f64; 3],
}

/// What one pass did: what it drew, and the engine's own account of the run.
#[derive(Debug, Default, Clone)]
pub struct Outcome {
    /// Bores the engine returned — every side of every joint, so a cam counts its bolt, its cup and its ⌀10 mate.
    pub holes: usize,
    /// Fasteners the engine fitted, i.e. distinct stations: 46 cams and 14 dowels are 60.
    pub stations: usize,
    /// Markers actually placed — one per station, unless the safety cap stopped the pass short.
    pub drawn: usize,
    /// Camlocks among the stations.
    pub cams: usize,
    /// Dowels among the stations. A dowel fitted beside a cam is its own fastener, so it is its own station.
    pub dowels: usize,
    /// Movable-shelf pins among the stations.
    pub pins: usize,
    /// Panels the translation accepted.
    pub panels: usize,
    /// Joints whose first position punched a neighbour and were moved (`CheckPunchConflict`).
    pub retried: usize,
    /// Seams refused outright.
    pub dropped: usize,
    /// Seams no count table covered.
    pub unbanded: usize,
    /// Positions the port has no emitter for yet.
    pub unported: usize,
    /// Positions a `自定义` hole string spelled but could not be parsed.
    pub unparsed: usize,
    /// The shop standard that drilled this run, by name. `None` is the vendor's count tables — what every run
    /// before a construction existed did, and what an item with no construction still gets.
    pub standard: Option<&'static str>,
    /// The one thing a reader of the count needs to know about the standard, stated by the pass: which board it
    /// had to take for the desk's modesty, or that it could not name one. Empty when nothing needed saying.
    pub rule: String,
}

/// `0 camlocks`, `1 camlock`, `46 camlocks` — the shop's plural, not a naive `s`.
fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

impl Outcome {
    /// The status line, in the ribbon's voice.
    ///
    /// Every count is always reported: the bores are the engine's answer, the fittings are what the shop ordered, and
    /// the stations are what the drawing now shows. A reader who sees only one of the three cannot tell a doubled joint
    /// from a missing one.
    pub fn message(&self) -> String {
        if self.panels == 0 {
            return "no boards to count - a flat-pack carcass needs two boards meeting".to_owned();
        }
        if self.holes == 0 {
            return format!("{} panels, no bore fitted on them", self.panels);
        }
        let mut fittings = format!("{} and {}", plural(self.cams, "camlock"), plural(self.dowels, "dowel"));
        if self.pins > 0 {
            fittings.push_str(&format!(" and {}", plural(self.pins, "shelf pin")));
        }
        let short = if self.drawn == self.stations { String::new() } else { format!(", {} markers drawn", self.drawn) };
        // Which rule set drilled this run, and the one thing about it a reader of the count needs: "eleven, not
        // four" is the same drawing under a different rule, so the rule has to be on the line.
        let mut rule = match self.standard {
            Some(standard) => format!("; rule set {standard}"),
            None => String::new(),
        };
        if !self.rule.is_empty() {
            rule.push_str(&format!("; {}", self.rule));
        }
        format!(
            "{} at {} stations ({} holes{}{}; engine: retried {}, dropped {}, unbanded {}, unported {}, unparsed {})",
            fittings, self.stations, self.holes, short, rule, self.retried, self.dropped, self.unbanded, self.unported, self.unparsed
        )
    }
}

/// Run the vendor's engine over the selected bodies - or all of them when nothing is selected - and draw one block
/// per station (fastener) it fits.
pub fn calc(app: &mut crate::SolveApp) -> Outcome {
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else {
        return Outcome::default();
    };
    let bodies = boxes(&measured);
    // What an earlier click drew is cleared first: a second press redraws the marks, it never adds a second set on top
    // of the first (88 marks had piled up in the 48x72 that way, and every one of them was still listed in the tree).
    let earlier: Vec<String> = bodies.iter().filter(|body| body.is_marker()).map(|body| body.name.clone()).collect();
    if !earlier.is_empty() {
        let _ = app.session.execute("solid.remove", &json!({ "bodies": earlier }));
    }
    // The folders go whether or not bodies were left behind: a pass whose markers were deleted by hand, or undone,
    // leaves its `Hardware` standing, and a second one beside it is exactly the stacking this pass must never make.
    clear_folders(app);
    // A selection narrows the pass to those bodies, which is what a person who picked two panels meant. Nothing selected
    // is every body in the drawing.
    let selected: BTreeSet<String> = app
        .session
        .selection
        .iter()
        .filter_map(|sel| match sel {
            solidcraft_engine::Sel::Body { name } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let boards: Vec<Board> = bodies
        .iter()
        .filter(|body| !body.is_marker() && (selected.is_empty() || selected.contains(&body.name)))
        .map(|body| Board {
            name: body.name.clone(),
            lo: body.lo,
            hi: body.hi,
            // The role a part was assigned in Ledger's parts window comes back through the engine's own rule for now:
            // the name first, then the box. A stored role is the next step, not this one.
            role: None,
            declared_role: None,
            link: None,
            hole_txt: None,
        })
        .collect();
    let mut translation = translate_boards(&boards);
    let mut panels = std::mem::take(&mut translation.panels);
    if panels.len() < 2 {
        return Outcome { panels: panels.len(), ..Outcome::default() };
    }
    let (run, standard, rule) = drill(&mut panels, &translation.roles);
    let mut drawn = 0usize;
    /// A carcass has tens of bores. Hundreds means the pairing is wrong, and drawing them would bury the design.
    const MOST_MARKERS: usize = 600;

    // **One marker per station, at the joint.** The engine says which fastener each bore belongs to
    // (`Hole::station`), so the list is grouped by that id rather than by proximity: the bolt and its ⌀10 mate are the
    // same world point, and one cam is three bores. Within a station the block goes on the bore that *is* the joint —
    // `ledger_holes::station_anchor` prefers the mate's pre-hole, i.e. the point where the two boards meet. That is
    // what makes a board which only receives mates (the tabletop: 22 pre-holes and 6 dowel mates, no cup at all)
    // carry a mark, and it puts the cup board's mark on the same point instead of 33 mm in from the seam; a corner
    // no longer collects a pile of blocks either.
    let mut stations: Vec<Station> = Vec::new();
    let mut bores: Vec<Vec<&Hole>> = Vec::new();
    for hole in &run.holes {
        if !panels.iter().any(|panel| panel.id == hole.part_id) {
            continue;
        }
        match stations.iter().position(|station| station.id == hole.station) {
            Some(index) => bores[index].push(hole),
            None => {
                stations.push(Station { id: hole.station, fastener: hole.kind.fastener(), at: [0.0; 3] });
                bores.push(vec![hole]);
            }
        }
    }
    for (station, group) in stations.iter_mut().zip(&bores) {
        let Some(anchor) = station_anchor(group.iter().copied()) else {
            continue;
        };
        let Some(panel) = panels.iter().find(|panel| panel.id == anchor.part_id) else {
            continue;
        };
        station.at = hole_at_world(panel, anchor);
        station.fastener = anchor.kind.fastener();
    }

    let mut filed: std::collections::BTreeMap<&'static str, Vec<String>> = std::collections::BTreeMap::new();
    let mut numbers: std::collections::BTreeMap<&'static str, usize> = std::collections::BTreeMap::new();
    for station in &stations {
        if drawn >= MOST_MARKERS {
            break;
        }
        let (folder, word) = family(station.fastener);
        let number = numbers.entry(word).or_insert(0);
        *number += 1;
        let name = format!("{word} {number}");
        let made = app.session.execute(
            "solid.box",
            &json!({
                "length": MARKER,
                "width": MARKER,
                "height": MARKER_DEEP,
                "center": station.at,
                "name": name,
                "body_name": name,
            }),
        );
        // Only what the kernel actually made is filed: a refused box must not leave a name in a folder, and a name the
        // kernel had to change must be filed under the name the tree now shows, or the folder points at nothing.
        let Ok(made) = made else { continue };
        let created: Vec<String> = made
            .get("created_bodies")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_else(|| vec![name]);
        if created.is_empty() {
            continue;
        }
        filed.entry(folder).or_default().extend(created);
        drawn += 1;
    }
    file_under_hardware(app, &filed);
    let count = |fastener: Fastener| stations.iter().filter(|station| station.fastener == fastener).count();
    Outcome {
        holes: run.holes.len(),
        stations: stations.len(),
        drawn,
        cams: count(Fastener::Cam),
        dowels: count(Fastener::Dowel),
        pins: count(Fastener::ShelfPin),
        panels: panels.len(),
        retried: run.retried,
        dropped: run.dropped,
        unbanded: run.unbanded,
        unported: run.unported,
        unparsed: run.unparsed,
        standard,
        rule,
    }
}

/// The engine's run over the drawing, with the item's **construction** choosing the rule set.
///
/// The boards say what each part is; Ledger's `current.json` — the item the editor was opened for — says what the
/// whole item is. A desk selects the shop's [`ledger_holes::table_rules::DKS_DESK`] standard, and that is the only
/// thing that turns the SOL-DKS drawing's four cams into the eleven the BOM calls for. Everything else (a cabinet, a
/// table, an island, or no construction at all — an editor started on its own) keeps the vendor's count tables, byte
/// for byte as before.
fn drill(panels: &mut [Panel], roles: &[Role]) -> (Plan, Option<&'static str>, String) {
    let Some(standard) = construction().and_then(Construction::standard) else {
        return (plan(panels, &Settings::default()), None, String::new());
    };
    run_with(panels, roles, standard)
}

/// The standard's run, and what a reader must know about the parts it was given.
///
/// The parts are read by [`desk_declarations`] — the board's own name first (`Top`, `Side - left`, `Full Modesty`),
/// then the role the translation read. The shop's imported drawings do not use those names: the STEP calls the
/// desk's modesty `panel - front` (the importer's word for a front-to-back panel), so the standard cannot name it,
/// and without it the desk is drilled by the four side-to-top joints alone. That board is found the way the BOM
/// describes the desk: it is the one plate that meets **both** sides and the top, and only when exactly one does.
fn run_with(panels: &mut [Panel], roles: &[Role], standard: &'static Standard) -> (Plan, Option<&'static str>, String) {
    let mut declared: Vec<Declared> = desk_declarations(panels, roles);
    let mut note = String::new();
    if !declared.iter().any(|entry| entry.part == DeskPart::Modesty) {
        match modesty(panels, &declared) {
            Some(id) => {
                let name = panels.iter().find(|panel| panel.id == id).map(|panel| panel.name.clone()).unwrap_or_default();
                declared.push(Declared::new(id, DeskPart::Modesty));
                note = format!("modesty `{name}`");
            }
            None => note = "no modesty panel could be named".to_owned(),
        }
    }
    // The modesty's contact set is one no vendor string spells, so its joints only exist once the standard gives it
    // the shop's `固定` where the drawing left it undecided.
    declare_desk_links(panels, &declared);
    let run = plan_with_standard(panels, &Settings::default(), standard, &declared);
    (run, Some(standard.name), note)
}

/// What the item is, from Ledger's `current.json` beside the boards: the word the owner set on the item.
///
/// The file belongs to Ledger, not to the editor, and it is only a hint: absent, half written, carrying another
/// format marker or a word the shop does not use all mean "no construction" — the vendor's tables, which is today's
/// behaviour. Nothing here may fail a pass or block the UI over a file the operator never sees.
fn construction() -> Option<Construction> {
    let text = std::fs::read_to_string(crate::ledger_boards::file().with_file_name("current.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("format").and_then(Value::as_str) != Some("ledger-current/1") {
        return None;
    }
    Construction::from_token(value.get("construction").and_then(Value::as_str)?)
}

/// Two boxes are within this distance of each other on every axis: a joint, or a hairline gap the export left.
const TOUCH: f64 = 1.0;

/// A panel's world box, from its own frame and extents — the geometry the engine drills, not the body it came from.
fn extent(panel: &Panel) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for sx in [-0.5, 0.5] {
        for sy in [-0.5, 0.5] {
            for sz in [-0.5, 0.5] {
                let at = panel.frame.to_world([sx * panel.width, sy * panel.height, sz * panel.thickness]);
                for i in 0..3 {
                    lo[i] = lo[i].min(at[i]);
                    hi[i] = hi[i].max(at[i]);
                }
            }
        }
    }
    (lo, hi)
}

/// Whether two boards meet: their boxes overlap, or are within [`TOUCH`] of each other, on all three axes.
fn meet(a: &([f64; 3], [f64; 3]), b: &([f64; 3], [f64; 3])) -> bool {
    (0..3).all(|i| a.0[i] <= b.1[i] + TOUCH && b.0[i] <= a.1[i] + TOUCH)
}

/// The desk's modesty, when no board's name says which one it is: the one undeclared plate that meets both sides
/// **and** the top.
///
/// The SOL-DKS BOM is one top, two sides and one modesty, and the standard's eleven cams are the modesty's three
/// joints plus the sides' four — so the board under the top, spanning between the two ends, *is* the modesty of the
/// desk the construction says this is. Exactly one, or none: two candidates means a carcass with a back panel as
/// well, and there the guess would be the mistake, so the standard is simply not applied to either and the status
/// line says it could not name one.
fn modesty(panels: &[Panel], declared: &[Declared]) -> Option<u32> {
    let boxes = |part: DeskPart| -> Vec<([f64; 3], [f64; 3])> {
        declared
            .iter()
            .filter(|entry| entry.part == part)
            .filter_map(|entry| panels.iter().find(|panel| panel.id == entry.panel))
            .map(extent)
            .collect()
    };
    let tops = boxes(DeskPart::Top);
    let sides = boxes(DeskPart::Side);
    if tops.is_empty() || sides.len() < 2 {
        return None;
    }
    let found: Vec<u32> = panels
        .iter()
        .filter(|panel| !declared.iter().any(|entry| entry.panel == panel.id))
        .filter(|panel| {
            let board = extent(panel);
            sides.iter().all(|side| meet(&board, side)) && tops.iter().any(|top| meet(&board, top))
        })
        .map(|panel| panel.id)
        .collect();
    (found.len() == 1).then(|| found[0])
}

/// Whether a browser group is one this pass owns: `Hardware` itself, or a kind folder inside it, **and** holding
/// nothing but this pass's own markers. A folder the operator made and named `Cams` is theirs, and is left alone.
fn is_hardware_folder(group: &solidcraft_engine::doc::BrowserGroup) -> bool {
    let named = group.name == HARDWARE || [Fastener::Cam, Fastener::Dowel, Fastener::ShelfPin].iter().any(|f| family(*f).0 == group.name);
    named && group.items.iter().all(|item| is_marker_name(item))
}

/// Remove the folders an earlier pass made, so a re-run never stacks a second `Hardware` beside the first. Every folder
/// of the set is removed in its own right: ungrouping a parent moves its children **up into its place**, so a child left
/// out of the list would survive as a stray `Cams` at the folder's own level.
fn clear_folders(app: &mut crate::SolveApp) {
    let component = app.session.active_component;
    let ids: Vec<u64> = app
        .session
        .doc
        .browser_groups
        .iter()
        .filter(|g| g.component == component && g.folder == "bodies" && is_hardware_folder(g))
        .map(|g| g.id)
        .collect();
    // A parent first would leave its kind folders to be swept too; either order ends with none of them standing.
    for id in ids {
        let _ = app.session.execute("browser.ungroup", &json!({ "group": id }));
    }
}

/// File what the pass drew: one `Hardware` folder in the drawing's Bodies, and a folder per kind nested inside it.
///
/// `browser.group` is the editor's own browser-folder command, so the folders are saved in the design and undone like
/// any other change; a group is keyed by body name, which is exactly what the markers are called. A kind with nothing
/// in it gets no folder — an empty `Holes (0)` row would be clutter pretending to be hardware.
fn file_under_hardware(app: &mut crate::SolveApp, filed: &std::collections::BTreeMap<&'static str, Vec<String>>) {
    if filed.is_empty() {
        return;
    }
    let component = app.session.active_component;
    let Ok(hardware) = app.session.execute("browser.group", &json!({ "folder": "bodies", "component": component, "name": HARDWARE })) else {
        return;
    };
    let Some(parent) = hardware.get("group").and_then(Value::as_u64) else {
        return;
    };
    for (folder, names) in filed {
        let _ = app.session.execute(
            "browser.group",
            &json!({ "folder": "bodies", "component": component, "name": folder, "parent": parent, "items": names }),
        );
    }
}
