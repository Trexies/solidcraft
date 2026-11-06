//! Calc Camlocks: the vendor's hole engine, run on the bodies the editor has open, and a block drawn at every bore.
//!
//! This is the pass the shop runs from Ledger's parts window, moved to where the design is — and it is now literally the
//! same code: `ledger-holes`, the crate Ledger's `src/vendor/` re-exports, which is the port of the vendor client's
//! `CreateHolesInfo` / `AddSanHeYiToPart` / `CheckPunchConflict` chain. The bodies' names and world boxes go in as
//! `translate::Board`s, the engine answers with the holes it would drill, and each hole becomes a `cam` block at its
//! mouth. No seam rule of our own is applied here; the count is the engine's.
//!
//! A bore is a block - orange, the size of the cutter - never a hole in the panel, because cutting would destroy the
//! drawing. The block is 15 x 15 x 14.2, the cam cup's own cutter, and it stands at the bore's mouth in model space: the
//! engine reports a hole in its panel's frame, and [`ledger_holes::hole_at_world`] undoes that frame (and the vendor's
//! `0..thickness` z scale) so the block lands where the panel is.

use std::collections::BTreeSet;

use ledger_holes::translate::Board;
use ledger_holes::{Panel, Settings, hole_at_world, plan};
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

/// What one pass did: what it drew, and the engine's own account of the run.
#[derive(Debug, Default, Clone, Copy)]
pub struct Outcome {
    /// Markers placed - one per hole the engine returned.
    pub holes: usize,
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
    pub fn message(&self) -> String {
        if self.panels == 0 {
            return "no boards to count - a flat-pack carcass needs two boards meeting".to_owned();
        }
        if self.holes == 0 {
            return format!("{} panels, no bore fitted on them", self.panels);
        }
        format!(
            "{} camlocks marked on {} panels (engine: retried {}, dropped {}, unbanded {}, unported {}, unparsed {})",
            self.holes, self.panels, self.retried, self.dropped, self.unbanded, self.unported, self.unparsed
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
    for hole in &run.holes {
        if drawn >= MOST_MARKERS {
            break;
        }
        let Some(panel) = panels.iter().find(|panel| panel.id == hole.part_id) else {
            continue;
        };
        cam_number += 1;
        let name = format!("cam {cam_number}");
        let centre = hole_at_world(panel, hole);
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
        holes: drawn,
        panels: panels.len(),
        retried: run.retried,
        dropped: run.dropped,
        unbanded: run.unbanded,
        unported: run.unported,
        unparsed: run.unparsed,
    }
}
