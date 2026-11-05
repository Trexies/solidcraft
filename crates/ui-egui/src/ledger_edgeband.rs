//! Merging edgebanding into the panel it belongs to, on demand.
//!
//! A DWG ships edgebanding as its own thin solid. Left alone each strip is a body of its own, which splits one panel into two and
//! doubles the hardware counted against it. The import pass joins them, but a band can arrive a hair off its panel, or be drawn
//! after the import - so the ribbon needs the same pass on a button.
//!
//! A band is: thin (a few millimetres across one axis), longer than it is thin in both other axes, and beside a thicker body it
//! overlaps in those two axes and nearly touches across the third. The pair is joined; a band with nothing to join is counted
//! and left where it is, because a union that guessed would invent a panel that is not there.

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

/// How thin a body has to be to count as a strip of edgebanding.
const BAND_MM: f64 = 4.0;
/// How far a band may sit from its panel and still be joined. A drawing often leaves a hairline, and a band drawn proud of the
/// face is still the band for that face.
const TOUCH_MM: f64 = 3.0;
/// How much of the panel the band has to run along before the pair is believed.
const OVERLAP_MM: f64 = 1.0;

fn boxes(value: &Value) -> Vec<Box3> {
    let mut out = Vec::new();
    let Some(list) = value.get("bodies").and_then(Value::as_array) else {
        return out;
    };
    for body in list {
        let Some(name) = body.get("name").and_then(Value::as_str) else { continue };
        let Some(bbox) = body.get("bbox") else { continue };
        let corner = |key: &str| -> Option<[f64; 3]> {
            let list = bbox.get(key)?.as_array()?;
            Some([list.first()?.as_f64()?, list.get(1)?.as_f64()?, list.get(2)?.as_f64()?])
        };
        let (Some(lo), Some(hi)) = (corner("min"), corner("max")) else { continue };
        out.push(Box3 { name: name.to_owned(), lo, hi });
    }
    out
}

/// Join every band that can be joined. Answers (merged, left alone).
pub fn merge(app: &mut crate::SolveApp) -> (usize, usize) {
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else {
        return (0, 0);
    };
    let bodies = boxes(&measured);
    if bodies.len() < 2 {
        return (0, 0);
    }
    // Thinnest first, so a band is joined to a panel rather than to another band.
    let mut order: Vec<usize> = (0..bodies.len()).collect();
    order.sort_by(|a, b| {
        let (sa, sb) = (bodies[*a].size(), bodies[*b].size());
        sa[0].min(sa[1]).min(sa[2]).total_cmp(&sb[0].min(sb[1]).min(sb[2]))
    });
    let mut gone = vec![false; bodies.len()];
    let mut merged = 0usize;
    let mut left = 0usize;
    for index in order {
        if gone[index] {
            continue;
        }
        let band = &bodies[index];
        let size = band.size();
        let mut axes = [0usize, 1, 2];
        axes.sort_by(|a, b| size[*a].total_cmp(&size[*b]));
        let thin_axis = axes[0];
        if size[thin_axis] > BAND_MM {
            continue;
        }
        // A band runs along at least one axis; a washer-sized chip is not edgebanding.
        if size[axes[1]] < 10.0 {
            continue;
        }
        let others = [(thin_axis + 1) % 3, (thin_axis + 2) % 3];
        let mut best: Option<(usize, f64)> = None;
        for (candidate, panel) in bodies.iter().enumerate() {
            if candidate == index || gone[candidate] {
                continue;
            }
            if panel.size()[thin_axis] <= BAND_MM {
                continue;
            }
            let gap = if band.hi[thin_axis] < panel.lo[thin_axis] {
                panel.lo[thin_axis] - band.hi[thin_axis]
            } else if panel.hi[thin_axis] < band.lo[thin_axis] {
                band.lo[thin_axis] - panel.hi[thin_axis]
            } else {
                0.0
            };
            if gap > TOUCH_MM {
                continue;
            }
            let mut shared = 1.0f64;
            for axis in others {
                let overlap = band.hi[axis].min(panel.hi[axis]) - band.lo[axis].max(panel.lo[axis]);
                if overlap < OVERLAP_MM {
                    shared = 0.0;
                    break;
                }
                shared *= overlap;
            }
            if shared > 0.0 && best.is_none_or(|(_, area)| shared > area) {
                best = Some((candidate, shared));
            }
        }
        let Some((panel, _)) = best else {
            left += 1;
            continue;
        };
        let joined = app
            .session
            .execute(
                "solid.combine",
                &json!({
                    "target": bodies[panel].name,
                    "tools": [band.name],
                    "operation": "join",
                }),
            )
            .is_ok();
        if joined {
            merged += 1;
            gone[index] = true;
        } else {
            left += 1;
        }
    }
    (merged, left)
}
