//! Counting camlocks from the seams in the model, and drawing a block at each bore.
//!
//! This is the pass the shop used to run from Ledger's parts window, moved to where the design is. It is Ledger's own seam
//! model - [`crate::ledger_joints`], a translation of its `src/joints.rs` - run over the bodies the editor has open, so a
//! cabinet answers the same count here as it does there: two cam bores on a seam, the first 20 mm from its near end and the
//! last pinned 40 mm from the far one, wherever two boards 12-30 mm thick meet edge to face or edge to edge.
//!
//! A bore is a `cam` block - orange, the width of the cutter - never a hole in the panel, because cutting would destroy the
//! drawing.

use crate::ledger_joints::{self, Panel};
use serde_json::{Value, json};

struct Box3 {
    name: String,
    lo: [f64; 3],
    hi: [f64; 3],
}

impl Box3 {
    fn size(&self) -> [f64; 3] {
        [self.hi[0] - self.lo[0], self.hi[1] - self.lo[1], self.hi[2] - self.lo[2]]
    }

    fn centre(&self) -> [f64; 3] {
        [(self.lo[0] + self.hi[0]) / 2.0, (self.lo[1] + self.hi[1]) / 2.0, (self.lo[2] + self.hi[2]) / 2.0]
    }

    /// A bore marker this button put in the design on an earlier click.
    fn is_marker(&self) -> bool {
        self.name.starts_with("cam ") || self.name.starts_with("dowel ")
    }
}

/// A board: a real thickness, and thin against its own middle dimension.
fn is_board(body: &Box3) -> bool {
    let mut size = body.size();
    size.sort_by(f64::total_cmp);
    size[0] >= 5.0 && size[0] <= 40.0 && size[0] <= size[1] * 0.30
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

/// Count the seams between boards, and draw a cam block at every bore the shop's rule puts on them.
/// Answers (cams drawn, seams found).
pub fn calc(app: &mut crate::SolveApp) -> (usize, usize) {
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else {
        return (0, 0);
    };
    let bodies = boxes(&measured);
    // Bores already in the design are kept, and nothing new is put close enough to touch one: two markers in the same place
    // read as one bore and hide each other in the wireframe. Only markers that were there before this pass count. A pass's
    // own seats are laid out exactly as Ledger lays them out, twins and all: the 48 x 72 table carries two coincident copies
    // of its outer shell, so its 88 seats stand at 70 distinct points, and dropping a twin would drop a bore the drawing has.
    let placed: Vec<[f64; 3]> = bodies.iter().filter(|body| body.is_marker()).map(Box3::centre).collect();
    // Panels are the boards, never the markers: every marker is a 15 mm block, and a block in the list would seam against the
    // boards it stands in.
    let panels: Vec<Panel> =
        bodies.iter().filter(|body| !body.is_marker() && is_board(body)).map(|body| Panel::new(&body.name, body.lo, body.hi)).collect();
    if panels.len() < 2 {
        return (0, 0);
    }
    let automatic = ledger_joints::automatic(&panels, &ledger_joints::Rules::shop());
    // Numbering continues from the bores already in the design: starting again at `cam 1` makes the kernel rename the new
    // bodies to `Body78` and they stop reading as cams at all.
    let mut cam_number = placed.len();
    let mut dowel_number = 0usize;
    let mut drawn = 0usize;
    let mut cams = 0usize;
    /// A carcass has tens of seams. Hundreds means the pairing is wrong, and drawing them would bury the design.
    const MOST_CAMS: usize = 600;
    for seat in &automatic.seats {
        if drawn >= MOST_CAMS {
            break;
        }
        // Nudged out along the seam so half of it stands proud of the boards: a marker buried in the joint is a marker
        // nobody can see.
        let mut centre = seat.at;
        for (axis, shift) in centre.iter_mut().zip(seat.axis) {
            *axis += shift * 9.0;
        }
        // Clearance: a bore needs its own space, so a seat closer than this to one already in the design is dropped.
        const CLEARANCE_MM: f64 = 20.0;
        let too_close = placed.iter().any(|seen| {
            let square: f64 = (0..3).map(|axis| (centre[axis] - seen[axis]).powi(2)).sum();
            square.sqrt() < CLEARANCE_MM
        });
        if too_close {
            continue;
        }
        // The cutter's size, and never smaller than the block this button has always drawn.
        let width = seat.dia.max(15.0);
        let height = seat.depth.max(14.2);
        let name = match seat.kind {
            "cam" => {
                cam_number += 1;
                cams += 1;
                format!("cam {cam_number}")
            }
            _ => {
                dowel_number += 1;
                format!("dowel {dowel_number}")
            }
        };
        let _ = app.session.execute(
            "solid.box",
            &json!({
                "length": width,
                "width": width,
                "height": height,
                "center": centre,
                "name": name,
                "body_name": name,
            }),
        );
        drawn += 1;
    }
    (cams, automatic.seams)
}
