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
//! # One marker per station, not per hole
//!
//! The engine answers for **both** sides of a joint: a cam is a ⌀8 bolt up the source's edge and the ⌀10 pre-hole it
//! needs in the panel it lands on, and those two mouths are the *same world point*. Drawn one per hole, a 166-hole
//! table comes out as 166 blocks lying in 106 places, half of them exactly on top of the other half. So the mouths are
//! clustered within [`STATION`] and one block is placed on each cluster: 166 holes, 106 stations. Nothing the engine
//! returned changes — this is presentation, and the status line reports both numbers so the difference is never
//! silent.

use std::collections::BTreeSet;

use ledger_holes::translate::Board;
use ledger_holes::{Panel, Settings, hole_at_world, plan};
use serde_json::{Value, json};

/// The cam cup's cutter: the block this button has always drawn.
const MARKER: f64 = 15.0;
const MARKER_DEEP: f64 = 14.2;

/// How close two mouths have to be to be the same station.
///
/// A joint's two sides are reported from two different panels' frames, so the same world point arrives through two
/// sets of dot products and can differ in the last bit. Half a millimetre is far below any real spacing between bores
/// in a carcass (the pitch is 32 mm) and far above that noise.
pub const STATION: f64 = 0.5;

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

/// What one pass did: what it drew, and the engine's own account of the run.
#[derive(Debug, Default, Clone, Copy)]
pub struct Outcome {
    /// Holes the engine returned — both sides of every joint, so a cam counts its bolt and its ⌀10 mate.
    pub holes: usize,
    /// Markers placed: one per station, i.e. per cluster of mouths within [`STATION`].
    pub stations: usize,
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

impl Outcome {
    /// The status line, in the ribbon's voice.
    ///
    /// Both counts are always reported: the holes are the engine's answer, the stations are what the drawing now
    /// shows, and a reader who sees only one of the numbers cannot tell a doubled joint from a missing one.
    pub fn message(&self) -> String {
        if self.panels == 0 {
            return "no boards to count - a flat-pack carcass needs two boards meeting".to_owned();
        }
        if self.holes == 0 {
            return format!("{} panels, no bore fitted on them", self.panels);
        }
        format!(
            "{} holes at {} stations (engine: retried {}, dropped {}, unbanded {}, unported {}, unparsed {})",
            self.holes, self.stations, self.retried, self.dropped, self.unbanded, self.unported, self.unparsed
        )
    }
}

/// Run the vendor's engine over the selected bodies - or all of them when nothing is selected - and draw a block at
/// every hole it returns.
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

    // **One marker per station.** The engine answers for both sides of a joint — the ⌀8 bolt up the edge and the ⌀10
    // it needs in the mate are the same world point — so its hole list holds each shared mouth twice. Cluster the
    // mouths within `STATION` and keep the first of each cluster; the engine's list itself is untouched.
    let mut stations: Vec<[f64; 3]> = Vec::new();
    for hole in &run.holes {
        let Some(panel) = panels.iter().find(|panel| panel.id == hole.part_id) else {
            continue;
        };
        let centre = hole_at_world(panel, hole);
        let already = stations
            .iter()
            .any(|at| (0..3).all(|axis| (at[axis] - centre[axis]).abs() <= STATION));
        if !already {
            stations.push(centre);
        }
    }

    for centre in &stations {
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
                "center": centre,
                "name": name,
                "body_name": name,
            }),
        );
        drawn += 1;
    }
    Outcome {
        holes: run.holes.len(),
        stations: drawn,
        panels: panels.len(),
        retried: run.retried,
        dropped: run.dropped,
        unbanded: run.unbanded,
        unported: run.unported,
        unparsed: run.unparsed,
    }
}
