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
//!
//! # The desk's modesty is read off the drawing
//!
//! The desk standard is keyed by *part* (top, side, modesty), and the shop's imported STEP calls every body
//! `Part (n)` — nothing in the drawing says which one is the modesty, and the owner should not have to say it either.
//! [`ledger_holes::read_desk`] reads the parts in the owner's order of authority: a part the shop assigned a role to
//! (the parts window, which travels in `current.json` as `"parts": [{"name": "Part (4)", "role": "modesty"}, …]`) is
//! what the owner said it is; the board's own name is next (`Top`, `Side - left`, `Full Modesty`); and where neither
//! says anything the **boxes** decide — the plate spanning between the two ends, its top edge under the top, is the
//! modesty of the desk the construction says this is. The status line quotes which of the three answered and names
//! the board, so eleven is traceable to a board rather than to a guess.

use std::collections::{BTreeMap, BTreeSet};

use ledger_holes::construction::{Construction, declare_desk_links, read_desk};
use ledger_holes::translate::{Board, Role, translate_boards};
use ledger_holes::{Fastener, Hole, Panel, Plan, Settings, Standard, hole_at_world, plan, plan_with_standard, station_anchor};
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
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
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
    let (panels, run, standard, rule) = match engine(app) {
        Ok(found) => found,
        Err(panels) => return Outcome { panels, ..Outcome::default() },
    };
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
    let placed: Vec<Vec<crate::ledger_fittings::Bore>> = bores
        .iter()
        .map(|group| {
            group
                .iter()
                .filter_map(|hole| panels.iter().find(|panel| panel.id == hole.part_id).map(|panel| crate::ledger_fittings::bore(panel, hole)))
                .collect()
        })
        .collect();
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
    let mut coloured: std::collections::BTreeMap<&'static str, Vec<String>> = std::collections::BTreeMap::new();
    for (station, group) in stations.iter().zip(&placed) {
        if drawn >= MOST_MARKERS {
            break;
        }
        let (folder, word) = family(station.fastener);
        let number = numbers.entry(word).or_insert(0);
        *number += 1;
        let name = format!("{word} {number}");
        // The fitting as it is fitted - a housing turned to its bolt, a dowel across the seam - from the station's own
        // bores; the plain block only where the engine left the station without the bore the drawing is placed from.
        let refs: Vec<&crate::ledger_fittings::Bore> = group.iter().collect();
        let created: Vec<String> = match crate::ledger_fittings::fitting_steps(station.fastener, &refs) {
            Some(steps) => match crate::ledger_fittings::draw(app, &name, steps) {
                Some(body) => vec![body],
                None => continue,
            },
            None => {
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
                // Only what the kernel actually made is filed: a refused box must not leave a name in a folder, and a
                // name the kernel had to change must be filed under the name the tree now shows.
                let Ok(made) = made else { continue };
                made.get("created_bodies")
                    .and_then(Value::as_array)
                    .map(|list| list.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                    .unwrap_or_else(|| vec![name])
            }
        };
        if created.is_empty() {
            continue;
        }
        coloured.entry(crate::ledger_fittings::colour(station.fastener)).or_default().extend(created.iter().cloned());
        filed.entry(folder).or_default().extend(created);
        drawn += 1;
    }
    for (colour, bodies) in &coloured {
        let _ = app.session.execute("appearance.assign", &json!({ "bodies": bodies, "color": colour }));
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

/// The engine's run over the bodies in the drawing - the selected ones, or all of them - leaving out what this tab
/// drew itself. `Err` carries how many panels there were when fewer than two (nothing can be joined).
fn engine(app: &mut crate::SolveApp) -> Result<(Vec<Panel>, Plan, Option<&'static str>, String), usize> {
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else {
        return Err(0);
    };
    let bodies = boxes(&measured);
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
    let current = current();
    // The roles Ledger publishes are the owner's word; a part nobody spoke about carries none and the rule decides.
    let declared_roles = current.as_ref().map(|current| &current.roles);
    let boards: Vec<Board> = bodies
        .iter()
        .filter(|body| !body.is_marker() && (selected.is_empty() || selected.contains(&body.name)))
        .map(|body| Board {
            name: body.name.clone(),
            lo: body.lo,
            hi: body.hi,
            role: None,
            // The word the parts window stored, matched by the body name Ledger published it under. `None` is
            // Automatic: the rule reads the name first, then the box, exactly as before.
            declared_role: declared_roles.and_then(|roles| roles.get(&body.name).copied()),
            link: None,
            hole_txt: None,
        })
        .collect();
    let mut translation = translate_boards(&boards);
    let mut panels = std::mem::take(&mut translation.panels);
    if panels.len() < 2 {
        return Err(panels.len());
    }
    let construction = current.as_ref().and_then(|current| current.construction);
    let (run, standard, rule) = drill(&mut panels, &translation.roles, construction);
    Ok((panels, run, standard, rule))
}

/// Calc Holes: cut every bore the engine drills into its board, replacing what an earlier press cut, and keep the list.
pub fn holes(app: &mut crate::SolveApp) -> crate::ledger_fittings::HolesOutcome {
    let replaced = crate::ledger_fittings::clear_bores(app);
    let (panels, run, _, _) = match engine(app) {
        Ok(found) => found,
        Err(panels) => {
            return crate::ledger_fittings::HolesOutcome {
                nothing: Some(if panels == 0 {
                    "no boards to drill".to_owned()
                } else {
                    "no boards to drill - holes join two boards, and only one is here".to_owned()
                }),
                replaced,
                ..Default::default()
            };
        }
    };
    let bores: Vec<crate::ledger_fittings::Bore> = run
        .holes
        .iter()
        .filter_map(|hole| panels.iter().find(|panel| panel.id == hole.part_id).map(|panel| crate::ledger_fittings::bore(panel, hole)))
        .collect();
    if bores.is_empty() {
        return crate::ledger_fittings::HolesOutcome {
            nothing: Some(format!("{} boards, no bore fitted on them", panels.len())),
            replaced,
            ..Default::default()
        };
    }
    let mut outcome = crate::ledger_fittings::cut(app, &bores);
    outcome.replaced = replaced;
    outcome
}

/// The engine's run over the drawing, with the item's **construction** choosing the rule set.
///
/// The boards say what each part is; Ledger's `current.json` — the item the editor was opened for — says what the
/// whole item is. A desk selects the shop's [`ledger_holes::table_rules::DKS_DESK`] standard, and that is the only
/// thing that turns the SOL-DKS drawing's four cams into the eleven the BOM calls for. Everything else (a cabinet, a
/// table, an island, or no construction at all — an editor started on its own) keeps the vendor's count tables, byte
/// for byte as before.
fn drill(panels: &mut [Panel], roles: &[Role], construction: Option<Construction>) -> (Plan, Option<&'static str>, String) {
    let Some(standard) = construction.and_then(Construction::standard) else {
        return (plan(panels, &Settings::default()), None, String::new());
    };
    run_with(panels, roles, standard)
}

/// The standard's run, and what a reader must know about the parts it was given.
///
/// The parts are read by [`read_desk`], in the engine both programs share: the role the shop assigned first (a part
/// recorded as `modesty` *is* the modesty, with no geometry test), then the board's own name, and — where the drawing
/// names nothing, which is every imported `Part (n)` — the boxes. Whichever answered is named on the status line
/// beside the board, so a count that changed can be traced to the board that changed it.
fn run_with(panels: &mut [Panel], roles: &[Role], standard: &'static Standard) -> (Plan, Option<&'static str>, String) {
    let read = read_desk(panels, roles);
    let name_of = |id: u32| -> String { panels.iter().find(|panel| panel.id == id).map(|panel| panel.name.clone()).unwrap_or_default() };
    // `role`, `name` or `geometric`: the word is the answer to "why is this board the modesty", and without it
    // eleven cams on a drawing nobody described would look like the button inventing a panel.
    let note = match &read.modesty {
        Some(found) => format!("modesty `{}` ({})", name_of(found.panel), found.source.word()),
        None => "no modesty panel could be named".to_owned(),
    };
    // The modesty's contact set is one no vendor string spells, so its joints only exist once the standard gives it
    // the shop's `固定` where the drawing left it undecided.
    declare_desk_links(panels, &read.declared);
    let run = plan_with_standard(panels, &Settings::default(), standard, &read.declared);
    (run, Some(standard.name), note)
}

/// What Ledger says about the item the editor was opened for.
///
/// `current.json` belongs to Ledger, not to the editor, and it is only a hint: absent, half written, carrying another
/// format marker or a word the shop does not use all mean "nothing declared" — the vendor's tables, which is today's
/// behaviour. Nothing here may fail a pass or block the UI over a file the operator never sees.
#[derive(Debug, Default)]
struct Current {
    /// `desk`, `table`, … — the word the owner set on the item (or the SKU gave it).
    construction: Option<Construction>,
    /// The role each part was assigned, by the body name Ledger published it under. An automatic part is absent.
    roles: BTreeMap<String, Role>,
}

fn current() -> Option<Current> {
    let text = std::fs::read_to_string(crate::ledger_boards::file().with_file_name("current.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("format").and_then(Value::as_str) != Some("ledger-current/1") {
        return None;
    }
    let mut roles = BTreeMap::new();
    for part in value.get("parts").and_then(Value::as_array).into_iter().flatten() {
        let (Some(name), Some(role)) = (part.get("name").and_then(Value::as_str), part.get("role").and_then(Value::as_str)) else {
            continue;
        };
        // A word this build does not know is ignored, not turned into a guess.
        if let Some(role) = Role::from_token(role) {
            roles.insert(name.to_owned(), role);
        }
    }
    Some(Current { construction: value.get("construction").and_then(Value::as_str).and_then(Construction::from_token), roles })
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
        let _ = app
            .session
            .execute("browser.group", &json!({ "folder": "bodies", "component": component, "name": folder, "parent": parent, "items": names }));
    }
}
