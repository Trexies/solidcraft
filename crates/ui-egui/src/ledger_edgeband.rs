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
    volume: f64,
}

impl Box3 {
    /// The space its box takes. Used instead of the model's own volume: imported bodies report negative volume when their
    /// faces face inward, and a negative number would make a real panel look like a shell.
    fn box_volume(&self) -> f64 {
        let size = self.size();
        size[0] * size[1] * size[2]
    }

    /// Is this body the same box as `other`, to within a hair? A band that wraps a panel's edge is modelled as a second,
    /// thinner solid on the panel's own outline, so the two boxes are the same.
    fn material(&self) -> f64 {
        self.volume.abs()
    }

    fn twin_of(&self, other: &Box3) -> bool {
        // Two solids for one panel are rarely the same box to the millimetre: the drawing puts the second one a couple of
        // millimetres proud, so the sizes differ by a few and the corners do not line up. What they do share is almost all of
        // their space, which is the test that matters.
        let (a, b) = (self.size(), other.size());
        if !(0..3).all(|axis| (a[axis] - b[axis]).abs() <= 5.0) {
            return false;
        }
        let mut overlap = 1.0;
        for axis in 0..3 {
            overlap *= (self.hi[axis].min(other.hi[axis]) - self.lo[axis].max(other.lo[axis])).max(0.0);
        }
        let smaller = self.box_volume().min(other.box_volume());
        smaller > 0.0 && overlap / smaller >= 0.6
    }

    fn size(&self) -> [f64; 3] {
        [self.hi[0] - self.lo[0], self.hi[1] - self.lo[1], self.hi[2] - self.lo[2]]
    }
}

/// How thin a body has to be to count as a strip of edgebanding.
const BAND_MM: f64 = 4.0;
/// How far a band may sit from its panel and still be joined. A drawing often leaves a hairline, and a band drawn proud of the
/// face is still the band for that face.
const TOUCH_MM: f64 = 3.0;
/// How far a band may sit from its panel. Drawings place a band a whole board's thickness away (25.4 mm) rather than flush, so
/// the reach has to cover that.
const MERGE_GAP_MM: f64 = 40.0;
/// How much of the panel the band has to run along before the pair is believed.
const OVERLAP_MM: f64 = 1.0;
/// How small both short sides of a profile have to be: an angle of edgebanding, not a leg (150 mm across) or a rail (100 mm).
const PROFILE_MM: f64 = 45.0;

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
        let volume = body.get("volume_mm3").and_then(Value::as_f64).unwrap_or(0.0);
        out.push(Box3 { name: name.to_owned(), lo, hi, volume });
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
        // A strip is thin across its box; a band running right round a panel is a ring, and its box is as big as the panel's.
        let strip = size[thin_axis] <= BAND_MM;
        // A duplicate shell shows itself as a second body on another body's exact box with less material in it: the drawing
        // carries two solids per panel, and the thinner one has to go into the fuller one.
        let twin = (0..bodies.len()).find(|other| {
            *other != index
                && !gone[*other]
                && band.twin_of(&bodies[*other])
                && bodies[*other].material() > band.material()
        });
        let ring = twin.is_some();
        // A band can also be an angle: 25 x 25 across, running the length of a panel. It is neither thin nor hollow - it fills
        // about half its box - but its cross-section is a profile, not a board, which is what the shop sees when it points at it.
        let profile = size[axes[0]] <= PROFILE_MM
            && size[axes[1]] <= PROFILE_MM
            && size[axes[2]] >= 5.0 * size[axes[1]].max(1.0)
            && size[axes[2]] >= 100.0;
        if !strip && !ring && !profile {
            continue;
        }
        // A band runs along at least one axis; a washer-sized chip is not edgebanding.
        if strip && size[axes[1]] < 10.0 {
            continue;
        }
        // Which panel a band belongs to is decided by **distance**, not by which axis it is thin on: a ring round a panel's
        // face, an angle along an edge and a strip drawn a board's thickness away from the panel it trims are all the same
        // question - what is the nearest thing bigger than me. Every earlier rule here assumed the band touched its panel, and
        // seven bands on one table proved that assumption wrong.
        if let Some(partner) = twin {
            let joined = app
                .session
                .execute(
                    "solid.combine",
                    &json!({
                        "target": bodies[partner].name,
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
            continue;
        }
        let band_volume = band.box_volume();
        let mut best: Option<(usize, f64)> = None;
        for (candidate, panel) in bodies.iter().enumerate() {
            if candidate == index || gone[candidate] || panel.box_volume() <= band_volume {
                continue;
            }
            // Box-to-box distance: zero when they touch, the axis gap otherwise.
            let mut square = 0.0f64;
            let mut touching = 0usize;
            for axis in 0..3 {
                let gap = if band.hi[axis] < panel.lo[axis] {
                    panel.lo[axis] - band.hi[axis]
                } else if panel.hi[axis] < band.lo[axis] {
                    band.lo[axis] - panel.hi[axis]
                } else {
                    touching += 1;
                    0.0
                };
                square += gap * gap;
            }
            let distance = square.sqrt();
            if distance > MERGE_GAP_MM {
                continue;
            }
            // At the same distance, the body sharing more of an axis with the band wins.
            let score = distance - touching as f64 * 0.01;
            if best.is_none_or(|(_, seen)| score < seen) {
                best = Some((candidate, score));
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
