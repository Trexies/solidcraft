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

/// What the pass did: bands joined into their panels, hollow shells removed, bands left alone,
/// and — of the joined panels — how many the heal could clean and how many seam faces and edges
/// it dissolved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub joined: usize,
    pub removed: usize,
    pub left: usize,
    pub healed: usize,
    pub faces_dissolved: usize,
    pub edges_dissolved: usize,
}

/// Join every band that can be joined.
///
/// A band drawn as a hollow shell shares faces with its panel, and the kernel has no closed solid to join: it returns nothing
/// however the pair is offered. Such a band is **removed** instead - it sits on the panel's own faces, so deleting it leaves the
/// panel exactly as drawn and no gap - and its count is reported, never hidden.
pub fn merge(app: &mut crate::SolveApp) -> Report {
    let mut report = Report::default();
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else {
        return report;
    };
    let bodies = boxes(&measured);
    if bodies.len() < 2 {
        return report;
    }
    // Thinnest first, so a band is joined to a panel rather than to another band.
    let mut order: Vec<usize> = (0..bodies.len()).collect();
    order.sort_by(|a, b| {
        let (sa, sb) = (bodies[*a].size(), bodies[*b].size());
        sa[0].min(sa[1]).min(sa[2]).total_cmp(&sb[0].min(sb[1]).min(sb[2]))
    });
    let mut gone = vec![false; bodies.len()];
    let mut merged = 0usize;
    let mut removed = 0usize;
    let mut left = 0usize;
    // The panels a band went into, each once: healed at the end of the pass, not per strip.
    let mut panels: Vec<String> = Vec::new();
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
            // A ring round the panel is rebuilt as the closed band it stands for and joined in, so the board comes out as one
            // body with one outline. Only a ring that does not wrap its panel falls back to joining what was drawn.
            let joined = match cap_ring(app, band, &bodies[partner]) {
                Some(done) => done,
                None => app
                    .session
                    .execute(
                        "solid.combine",
                        &json!({
                            "target": bodies[partner].name,
                            "tools": [band.name],
                            "operation": "join",
                        }),
                    )
                    .is_ok(),
            };
            if joined {
                merged += 1;
                gone[index] = true;
                if !panels.contains(&bodies[partner].name) {
                    panels.push(bodies[partner].name.clone());
                }
            } else if is_shell(band) {
                // Coincident and hollow: there is no solid to join, and removing it leaves the panel exactly as drawn.
                let gone_ok = app
                    .session
                    .execute("solid.remove", &json!({ "bodies": [band.name] }))
                    .is_ok();
                if gone_ok {
                    removed += 1;
                    gone[index] = true;
                } else {
                    left += 1;
                }
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
            if !panels.contains(&bodies[panel].name) {
                panels.push(bodies[panel].name.clone());
            }
        } else if is_shell(band) {
            let gone_ok = app
                .session
                .execute("solid.remove", &json!({ "bodies": [band.name] }))
                .is_ok();
            if gone_ok {
                removed += 1;
                gone[index] = true;
            } else {
                left += 1;
            }
        } else {
            left += 1;
        }
    }
    // A ring was capped before its join (see `cap_ring`), so its board already has one outline. What is left for the heal is the
    // seam a strip or profile leaves where its faces meet the panel's on one plane: coplanar in fact, but split. Healing dissolves
    // those faces into one and joins the edges between them. A band drawn proud of the face is a real step and stays one - the
    // kernel only merges what lies on one plane.
    for name in &panels {
        let Ok(reply) = app.session.execute("solid.heal", &json!({ "body": name })) else {
            continue;
        };
        let dissolved = |key: &str| reply.get(key).and_then(Value::as_u64).unwrap_or(0) as usize;
        report.healed += 1;
        report.faces_dissolved += dissolved("faces_dissolved");
        report.edges_dissolved += dissolved("edges_dissolved");
    }
    report.joined = merged;
    report.removed = removed;
    report.left = left;
    report
}

/// A band that fills only part of its own box is a hollow shell - the drawing's way of drawing an edge, not a solid strip.
/// Volumes are compared by magnitude: imported bodies report negative volume when their faces face inward.
fn is_shell(band: &Box3) -> bool {
    let box_volume = band.box_volume();
    box_volume > 0.0 && band.material() < box_volume * 0.7
}

/// The box a ring of edgebanding really fills, when it wraps its panel: the ring's outline across the panel's face, and the
/// panel's own thickness through it. `None` when the ring does not go round the panel.
///
/// The shop's DWGs draw a banded board as a core inset by the band's thickness, plus a picture-frame ring of four side faces
/// with no top or bottom. Joined as drawn, that ring leaves zero-area faces and the core's inset outline on the board - the line
/// that stays visible in the edges view - because there is nothing on the top and bottom to merge.
fn ring_cap(band: &Box3, panel: &Box3) -> Option<([f64; 3], [f64; 3])> {
    let (rs, ps) = (band.size(), panel.size());
    // Through the board the ring and the core are the same thickness; that is the axis that differs least.
    let thick = (0..3).min_by(|a, b| (rs[*a] - ps[*a]).abs().total_cmp(&(rs[*b] - ps[*b]).abs()))?;
    let shared = band.hi[thick].min(panel.hi[thick]) - band.lo[thick].max(panel.lo[thick]);
    if shared <= ps[thick] * 0.5 {
        return None;
    }
    // Across the face the ring reaches round the core on every side.
    const HAIR: f64 = 0.01;
    for axis in (0..3).filter(|a| *a != thick) {
        if band.lo[axis] > panel.lo[axis] + HAIR || band.hi[axis] < panel.hi[axis] - HAIR {
            return None;
        }
    }
    let (mut lo, mut hi) = (band.lo, band.hi);
    lo[thick] = panel.lo[thick];
    hi[thick] = panel.hi[thick];
    Some((lo, hi))
}

/// Replace a ring of edgebanding with the closed band it stands for, and join that into the panel: a box on the ring's outline
/// with the panel's box cut out of it. Its faces lie exactly on the panel's, so the join leaves one board with one outline, and
/// a hole or groove in the panel is kept because the cap never covers the panel itself.
///
/// `None` when the ring does not wrap the panel (the caller joins it as drawn). `Some(false)` when the editor refused a step; the
/// design is then left as it was, the drawn ring included.
fn cap_ring(app: &mut crate::SolveApp, band: &Box3, panel: &Box3) -> Option<bool> {
    let (lo, hi) = ring_cap(band, panel)?;
    let cap = format!("{} cap", band.name);
    let opening = format!("{} opening", band.name);
    let block = |name: &str, lo: [f64; 3], hi: [f64; 3]| {
        json!({
            "length": hi[0] - lo[0],
            "width": hi[1] - lo[1],
            "height": hi[2] - lo[2],
            "corner": lo,
            "body_name": name,
        })
    };
    let steps = [
        ("solid.box", block(&cap, lo, hi)),
        ("solid.box", block(&opening, panel.lo, panel.hi)),
        ("solid.combine", json!({ "target": cap, "tools": [opening], "operation": "cut" })),
        ("solid.combine", json!({ "target": panel.name, "tools": [cap], "operation": "join" })),
        ("solid.remove", json!({ "bodies": [band.name] })),
    ];
    for (done, (command, params)) in steps.iter().enumerate() {
        if app.session.execute(command, params).is_err() {
            // Take back what this ring added, so a refusal never leaves a stray cap or opening in the design.
            let made: Vec<&str> = match done {
                0 => vec![],
                1 => vec![&cap],
                2 => vec![&cap, &opening],
                3 => vec![&cap],
                _ => vec![],
            };
            if !made.is_empty() {
                let _ = app.session.execute("solid.remove", &json!({ "bodies": made }));
            }
            return Some(false);
        }
    }
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(name: &str, lo: [f64; 3], hi: [f64; 3]) -> Box3 {
        Box3 { name: name.into(), lo, hi, volume: 0.0 }
    }

    /// SOL-DKS-2448's top as the DWG draws it: a core inset 2 mm all round and a capless ring on the outline. The cap is the
    /// ring's outline at the core's thickness - the whole 1200.15 x 600.07 x 25.4 top.
    #[test]
    fn a_ring_round_its_core_caps_to_the_whole_board() {
        let ring = body("Part", [0.0, -600.07, 736.6], [1200.15, 0.0, 762.0]);
        let core = body("Part (2)", [2.0, -598.07, 736.6], [1198.15, -2.0, 762.0]);
        let (lo, hi) = ring_cap(&ring, &core).expect("the ring wraps its core");
        assert_eq!((lo, hi), ([0.0, -600.07, 736.6], [1200.15, 0.0, 762.0]));
    }

    /// A leg's ring is drawn 0.1 mm thicker than its core on each side; the cap takes the core's thickness, so the board is not
    /// left with a step where the ring stood proud.
    #[test]
    fn the_cap_is_as_thick_as_the_core() {
        let ring = body("Part (11)", [1.5, -600.1, 7.0], [20.75, -0.02, 736.59]);
        let core = body("Part (12)", [1.6, -598.1, 9.0], [20.65, -2.03, 734.59]);
        let (lo, hi) = ring_cap(&ring, &core).expect("the ring wraps its core");
        assert_eq!((lo[0], hi[0]), (1.6, 20.65));
        assert_eq!((lo[1], hi[1], lo[2], hi[2]), (-600.1, -0.02, 7.0, 736.59));
    }

    /// The far leg's ring is the same size as this core but stands 1,177 mm away: not this core's ring.
    #[test]
    fn a_ring_beside_the_core_is_not_its_ring() {
        let ring = body("Part (11)", [1.6, -600.1, 7.0], [20.65, -0.02, 736.59]);
        let core = body("Part (10)", [1179.1, -598.1, 9.0], [1198.15, -2.03, 734.59]);
        assert!(ring_cap(&ring, &core).is_none());
    }

    /// A ring that stops short of one side of the core does not wrap it, and is joined as drawn.
    #[test]
    fn a_ring_short_of_the_core_is_not_capped() {
        let ring = body("Band", [3.0, -600.0, 0.0], [1200.0, 0.0, 25.4]);
        let core = body("Core", [2.0, -598.0, 0.0], [1198.0, -2.0, 25.4]);
        assert!(ring_cap(&ring, &core).is_none());
    }
}
