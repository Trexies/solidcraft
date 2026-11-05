//! Counting camlocks from the seams in the model, and drawing a block at each bore.
//!
//! This is the pass the shop used to run from Ledger's parts window, moved to where the design is: the seams are worked out from
//! the panels' own boxes, so a cabinet with fifteen panels and a hundred seams answers in a moment without a round trip.
//!
//! The rules are the shop's: first bore 20 mm from the end of the seam, then every 32 mm, and the last pinned 40 mm from the far
//! end; the bore sits 9 mm in from the panel's face. A bore is a `cam` block - orange, the width of the cutter - never a hole in
//! the panel, because cutting would destroy the drawing.

use serde_json::{json, Value};

struct Box3 {
    name: String,
    lo: [f64; 3],
    hi: [f64; 3],
}

impl Box3 {
    fn size(&self) -> [f64; 3] {
        [self.hi[0] - self.lo[0], self.hi[1] - self.lo[1], self.hi[2] - self.lo[2]]
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
        let (Some(name), Some(bbox)) = (
            body.get("name").and_then(Value::as_str),
            body.get("bbox"),
        ) else {
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
    let bodies: Vec<Box3> = boxes(&measured).into_iter().filter(is_board).collect();
    if bodies.len() < 2 {
        return (0, 0);
    }
    let mut cams = 0usize;
    let mut seams = 0usize;
    for a in 0..bodies.len() {
        for b in (a + 1)..bodies.len() {
            let (one, two) = (&bodies[a], &bodies[b]);
            // The seam runs along the axis they share most; they must meet across a different one.
            let mut shared_axis = 0usize;
            let mut shared = f64::MIN;
            for axis in 0..3 {
                let overlap = one.hi[axis].min(two.hi[axis]) - one.lo[axis].max(two.lo[axis]);
                if overlap > shared {
                    shared = overlap;
                    shared_axis = axis;
                }
            }
            if shared < 60.0 {
                continue;
            }
            // Which axis they meet across: a gap no wider than a hair, and no overlap on that axis.
            let mut meet = None;
            for axis in 0..3 {
                if axis == shared_axis {
                    continue;
                }
                let gap = if one.hi[axis] < two.lo[axis] {
                    two.lo[axis] - one.hi[axis]
                } else if two.hi[axis] < one.lo[axis] {
                    one.lo[axis] - two.hi[axis]
                } else {
                    0.0
                };
                let overlap = one.hi[axis].min(two.hi[axis]) - one.lo[axis].max(two.lo[axis]);
                if (0.0..=1.0).contains(&gap) && overlap > 0.0 {
                    meet = Some(axis);
                    break;
                }
            }
            let Some(meet) = meet else { continue };
            seams += 1;
            // The seam's own line: where the two boxes agree on the meeting axis.
            let plane = (one.hi[meet] + two.lo[meet]) / 2.0;
            let start = one.lo[shared_axis].max(two.lo[shared_axis]);
            let end = one.hi[shared_axis].min(two.hi[shared_axis]);
            let length = end - start;
            let mut at = if length <= 20.0 + 40.0 {
                vec![start + length / 2.0]
            } else {
                let mut spots = Vec::new();
                let mut position = 20.0;
                while position <= length - 40.0 {
                    spots.push(start + position);
                    position += 32.0;
                }
                spots.push(end - 40.0);
                spots
            };
            at.sort_by(f64::total_cmp);
            at.dedup_by(|x, y| (*x - *y).abs() < 1.0);
            // The other axis of the seam is the panel's own face; the bore sits 9 mm in from it.
            let across_axis = 3 - shared_axis - meet;
            let face = if one.size()[across_axis] < two.size()[across_axis] {
                one.lo[across_axis] + 9.0
            } else {
                two.lo[across_axis] + 9.0
            };
            for position in at {
                let mut centre = [0.0f64; 3];
                centre[shared_axis] = position;
                centre[meet] = plane;
                centre[across_axis] = face;
                cams += 1;
                let name = format!("cam {cams}");
                let _ = app.session.execute(
                    "solid.box",
                    &json!({
                        "length": 15.0,
                        "width": 15.0,
                        "height": 14.2,
                        "center": centre,
                        "name": name,
                        "body_name": name,
                    }),
                );
            }
        }
    }
    (cams, seams)
}
