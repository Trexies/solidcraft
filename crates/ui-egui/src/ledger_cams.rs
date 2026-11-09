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
//! placed once per station — 60 of them on the 48x72's 166 bores — and on the station's **cup** where it has one,
//! because that is the bore this block is the cutter of and it stands 33 mm in from the edge the joint converges on.
//! Nothing the engine returned changes; the status line states the bores, the fittings and the stations so the three
//! numbers can never be mistaken for each other.

use std::collections::BTreeSet;

use ledger_holes::translate::Board;
use ledger_holes::{Fastener, HoleKind, Panel, Settings, hole_at_world, plan};
use serde_json::{Value, json};

/// The cam cup's cutter: the block this button has always drawn.
const MARKER: f64 = 15.0;
const MARKER_DEEP: f64 = 14.2;

struct Box3 {
    name: String,
    lo: [f64; 3],
    hi: [f64; 3],
}

impl Box3 {
    /// A bore marker this button put in the design on an earlier click.
    fn is_marker(&self) -> bool {
        self.name.starts_with("cam ") || self.name.starts_with("dowel ")
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
#[derive(Debug, Default, Clone, Copy)]
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
        format!(
            "{} at {} stations ({} holes{}; engine: retried {}, dropped {}, unbanded {}, unported {}, unparsed {})",
            fittings, self.stations, self.holes, short, self.retried, self.dropped, self.unbanded, self.unported, self.unparsed
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
    // Bores already in the design are kept, and they are not boards: a marker in the list would seam against the boards
    // it stands in. They also decide where the new numbering starts - see below.
    let existing = bodies.iter().filter(|body| body.is_marker()).count();
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
    let panels: Vec<Panel> = ledger_holes::translate::panels_from_boards(&boards);
    if panels.len() < 2 {
        return Outcome { panels: panels.len(), ..Outcome::default() };
    }
    let run = plan(&panels, &Settings::default());
    // Numbering continues from the bores already in the design: starting again at `cam 1` makes the kernel rename the new
    // bodies to `Body78` and they stop reading as cams at all.
    let mut cam_number = existing;
    let mut drawn = 0usize;
    /// A carcass has tens of bores. Hundreds means the pairing is wrong, and drawing them would bury the design.
    const MOST_MARKERS: usize = 600;

    // **One marker per station.** The engine says which fastener each bore belongs to (`Hole::station`), so the list is
    // grouped by that id rather than by proximity: the bolt and its ⌀10 mate are the same world point, and one cam is
    // three bores. The block stands on the **cup** where the fitting has one — the 15 x 15 x 14.2 block *is* the cup's
    // cutter, and the cup is 33 mm in from the edge the joint converges on, so a corner no longer collects a pile of
    // blocks on one point.
    let mut stations: Vec<Station> = Vec::new();
    for hole in &run.holes {
        let Some(panel) = panels.iter().find(|panel| panel.id == hole.part_id) else {
            continue;
        };
        let at = hole_at_world(panel, hole);
        match stations.iter_mut().find(|station| station.id == hole.station) {
            Some(station) => {
                if hole.kind == HoleKind::CamCup {
                    station.at = at;
                }
            }
            None => stations.push(Station { id: hole.station, fastener: hole.kind.fastener(), at }),
        }
    }

    for station in &stations {
        if drawn >= MOST_MARKERS {
            break;
        }
        cam_number += 1;
        let name = format!("cam {cam_number}");
        let _ = app.session.execute(
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
        drawn += 1;
    }
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
    }
}
