//! Changing a board's thickness, and closing the joints it leaves open.
//!
//! Skinning a 1" side in a 1/2" board used to change its colour and nothing else: the drawing still said 1". Now a skin
//! whose thickness differs from the board's opens a preview of what a real swap would do, and applies it on request:
//!
//! * the board is thinned (or thickened) on its **inside** - the face nearest the outside of the whole piece stays where
//!   it is, so the product keeps its overall size (the owner's rule, 2026-11-12);
//! * every board that butted against the face that moved is stretched (or shortened) by the same amount so the joint
//!   closes again - the modesty grows to meet a thinner side. A board lying face-to-face against it (a doubled panel)
//!   is moved instead, since stretching it would change *its* thickness;
//! * nothing moves until the preview is accepted, and the whole change is taken back if any step is refused.
//!
//! The geometry is SolidCraft's own `solid.offset_face`, which keeps the body, its name, material, grain turn and sheet
//! record. Shop boards are boxes, so the plan is worked out on boxes ([`plan`]) and tested on the real desk's numbers.

use serde_json::{Value, json};

/// How close two faces must be to count as touching, in mm. Imported drawings are off by hundredths.
const TOUCH: f64 = 0.6;
/// How much two boards must share across a face to be joined there, in mm - not just meet at a corner.
const SHARE: f64 = 1.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Box3 {
    pub name: String,
    pub lo: [f64; 3],
    pub hi: [f64; 3],
}

impl Box3 {
    fn size(&self, axis: usize) -> f64 {
        self.hi[axis] - self.lo[axis]
    }
    fn thin_axis(&self) -> usize {
        (0..3).min_by(|a, b| self.size(*a).total_cmp(&self.size(*b))).unwrap_or(2)
    }
}

/// One step of a resize.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Offset the face of `body` at `at` by `distance` mm (positive: outward, the body grows).
    Offset { body: String, at: [f64; 3], distance: f64 },
    /// Move `body` by `by`.
    Shift { body: String, by: [f64; 3] },
}

/// What a resize will do, in steps and in words.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub lines: Vec<String>,
}

const AXIS: [&str; 3] = ["X", "Y", "Z"];

fn overlap(a: &Box3, b: &Box3, axis: usize) -> f64 {
    a.hi[axis].min(b.hi[axis]) - a.lo[axis].max(b.lo[axis])
}

/// Make `target` `thickness` mm thick, keeping its outside face, and close the joints on its inside. `boxes` is every
/// board in the design (markers left out); `target` must be one of them. Answers the plan and the boxes as they will
/// be after it, so several boards can be planned one after another.
pub fn plan(boxes: &[Box3], target: &str, thickness: f64) -> Result<(Plan, Vec<Box3>), String> {
    let me = boxes.iter().find(|b| b.name == target).ok_or_else(|| format!("{target} is not in the design"))?;
    let axis = me.thin_axis();
    let old = me.size(axis);
    let delta = old - thickness; // > 0: thinner
    if delta.abs() < 0.05 {
        return Ok((Plan::default(), boxes.to_vec()));
    }
    if thickness <= 0.5 {
        return Err(format!("{target} cannot be {thickness:.2} mm thick"));
    }
    // An edgebanding ring around the board has the board's box: resizing the core under it would leave the ring standing.
    if let Some(ring) = boxes.iter().find(|b| {
        b.name != me.name && (0..3).all(|k| overlap(b, me, k) > me.size(k) * 0.8 - TOUCH) && b.size(axis) > old * 0.5
    }) {
        return Err(format!(
            "{target} sits inside {} - merge its edgebanding first (Merge edgebanding), then change its thickness",
            ring.name
        ));
    }
    // The outside face is the one nearer the outside of the whole piece along the thin axis.
    let lo = boxes.iter().map(|b| b.lo[axis]).fold(f64::INFINITY, f64::min);
    let hi = boxes.iter().map(|b| b.hi[axis]).fold(f64::NEG_INFINITY, f64::max);
    let keep_lo = (me.lo[axis] - lo) <= (hi - me.hi[axis]);
    // The face that moves, where it is, and which way it moves (toward the kept face when thinning).
    let (plane, toward) = if keep_lo { (me.hi[axis], -1.0) } else { (me.lo[axis], 1.0) };
    let shift = toward * delta;
    let new_plane = plane + shift;

    let centre_on = |b: &Box3, at: f64| -> [f64; 3] {
        let mut p = [0.0; 3];
        for k in 0..3 {
            p[k] = if k == axis { at } else { (b.lo[k] + b.hi[k]) / 2.0 };
        }
        p
    };
    let mut steps = vec![Step::Offset { body: me.name.clone(), at: centre_on(me, plane), distance: -delta }];
    let mut lines = vec![format!(
        "{}: {:.2} → {:.2} mm thick, outside face kept",
        me.name, old, thickness
    )];
    let mut after = boxes.to_vec();
    if let Some(b) = after.iter_mut().find(|b| b.name == me.name) {
        if keep_lo { b.hi[axis] = new_plane } else { b.lo[axis] = new_plane }
    }
    for other in boxes.iter().filter(|b| b.name != me.name) {
        // Touching the moving face from the inside, and sharing more than a corner with it.
        let face = if keep_lo { other.lo[axis] } else { other.hi[axis] };
        if (face - plane).abs() > TOUCH {
            continue;
        }
        let shared = (0..3).filter(|k| *k != axis).all(|k| overlap(other, me, k) > SHARE);
        if !shared {
            continue;
        }
        let entry = after.iter_mut().find(|b| b.name == other.name).expect("same boxes");
        if other.thin_axis() == axis {
            let mut by = [0.0; 3];
            by[axis] = shift;
            steps.push(Step::Shift { body: other.name.clone(), by });
            entry.lo[axis] += shift;
            entry.hi[axis] += shift;
            lines.push(format!("{}: moved {:.2} mm along {} to stay against it", other.name, delta.abs(), AXIS[axis]));
        } else {
            let mut p = [0.0; 3];
            for k in 0..3 {
                p[k] = if k == axis {
                    face
                } else {
                    // On the shared part of the face, so the point is on this board's face and nowhere else.
                    (other.lo[k].max(me.lo[k]) + other.hi[k].min(me.hi[k])) / 2.0
                };
            }
            steps.push(Step::Offset { body: other.name.clone(), at: p, distance: delta });
            let was = other.size(axis);
            if keep_lo { entry.lo[axis] = new_plane } else { entry.hi[axis] = new_plane }
            lines.push(format!(
                "{}: {:.1} → {:.1} mm along {} ({} {:.2} mm to meet it)",
                other.name,
                was,
                was + delta,
                AXIS[axis],
                if delta > 0.0 { "grows" } else { "shrinks" },
                delta.abs()
            ));
        }
    }
    if lines.len() == 1 {
        lines.push("nothing butts against its inside face, so no other board changes".to_owned());
    }
    Ok((Plan { steps, lines }, after))
}

/// Every board in the design as a box, markers left out.
pub fn boxes(app: &mut crate::SolveApp) -> Vec<Box3> {
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for body in measured.get("bodies").and_then(Value::as_array).into_iter().flatten() {
        let (Some(name), Some(bbox)) = (body.get("name").and_then(Value::as_str), body.get("bbox")) else { continue };
        if crate::ledger_cams::is_marker_name(name) {
            continue;
        }
        let corner = |key: &str| -> Option<[f64; 3]> {
            let v = bbox.get(key)?.as_array()?;
            Some([v.first()?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?])
        };
        let (Some(lo), Some(hi)) = (corner("min"), corner("max")) else { continue };
        out.push(Box3 { name: name.to_owned(), lo, hi });
    }
    out
}

/// The bodies a selection points at: a picked body, or the body a picked face, edge or corner is on.
pub fn selected(app: &crate::SolveApp) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for sel in &app.session.selection {
        let name = match sel {
            solidcraft_engine::Sel::Body { name } => name,
            solidcraft_engine::Sel::Face { body, .. }
            | solidcraft_engine::Sel::Edge { body, .. }
            | solidcraft_engine::Sel::Vertex { body, .. } => body,
            _ => continue,
        };
        if !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
}

/// Plan every chosen board in turn, each against the design as the earlier ones leave it.
pub fn plan_all(boxes: &[Box3], bodies: &[String], thickness: f64) -> Result<Plan, String> {
    let mut all = Plan::default();
    let mut now = boxes.to_vec();
    for body in bodies {
        let (plan, after) = plan(&now, body, thickness)?;
        all.steps.extend(plan.steps);
        all.lines.extend(plan.lines);
        now = after;
    }
    Ok(all)
}

/// The boxes as [`plan_all`] would leave them, for a dry run of the hole engine.
pub fn after_all(boxes: &[Box3], bodies: &[String], thickness: f64) -> Result<Vec<Box3>, String> {
    let mut now = boxes.to_vec();
    for body in bodies {
        now = plan(&now, body, thickness)?.1;
    }
    Ok(now)
}

/// Carry a plan out. All or nothing: a refused step puts the design back as it was.
pub fn apply(app: &mut crate::SolveApp, plan: &Plan) -> Result<(), String> {
    let mark = app.session.checkpoint();
    for step in &plan.steps {
        let done = match step {
            Step::Offset { body, at, distance } => app.session.execute(
                "solid.offset_face",
                &json!({ "faces": [at], "distance": distance, "body": body }),
            ),
            Step::Shift { body, by } => {
                app.session.execute("solid.move", &json!({ "bodies": [body], "translate": by }))
            }
        };
        if let Err(error) = done {
            let _ = app.session.restore_checkpoint(mark);
            let who = match step {
                Step::Offset { body, .. } | Step::Shift { body, .. } => body,
            };
            return Err(format!("{who} could not be resized ({error}) - nothing was changed"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, lo: [f64; 3], hi: [f64; 3]) -> Box3 {
        Box3 { name: name.into(), lo, hi }
    }

    /// SOL-DKS-2448 as it measures: top, both sides, modesty, feet.
    fn desk() -> Vec<Box3> {
        vec![
            b("top", [0.0, -600.07, 736.6], [1200.15, 0.0, 762.0]),
            b("panel - front", [20.64, -590.55, 7.01], [1179.12, -571.5, 736.6]),
            b("side", [1179.12, -600.07, 7.01], [1198.17, 0.0, 736.6]),
            b("side (2)", [1.59, -600.07, 7.01], [20.64, 0.0, 736.6]),
            b("foot - front left", [0.4, -458.3, -1.8], [21.8, -437.0, 8.8]),
        ]
    }

    /// The left side from 3/4" to 1/2": its outside face stays, the modesty grows 6.35 mm to meet it, nothing else moves.
    #[test]
    fn a_thinner_side_keeps_its_outside_and_the_modesty_follows() {
        let (plan, after) = plan(&desk(), "side (2)", 12.7).unwrap();
        let side = after.iter().find(|b| b.name == "side (2)").unwrap();
        assert!((side.lo[0] - 1.59).abs() < 1e-9, "outside face kept");
        assert!((side.hi[0] - 14.29).abs() < 0.01, "{side:?}");
        let modesty = after.iter().find(|b| b.name == "panel - front").unwrap();
        assert!((modesty.lo[0] - 14.29).abs() < 0.01, "the modesty meets it: {modesty:?}");
        assert_eq!(plan.steps.len(), 2, "{:?}", plan.steps);
        // The top sits on the side's edge, the foot under it: neither touches the face that moved.
        assert_eq!(after.iter().find(|b| b.name == "top"), desk().iter().find(|b| b.name == "top"));
        match &plan.steps[0] {
            Step::Offset { distance, at, .. } => {
                assert!((distance + 6.35).abs() < 0.01);
                assert!((at[0] - 20.64).abs() < 1e-9, "on the inside face");
            }
            other => panic!("{other:?}"),
        }
    }

    /// The right side keeps its right face, which is its outside.
    #[test]
    fn the_other_side_keeps_its_own_outside() {
        let (_, after) = plan(&desk(), "side", 12.7).unwrap();
        let side = after.iter().find(|b| b.name == "side").unwrap();
        assert!((side.hi[0] - 1198.17).abs() < 1e-9 && (side.lo[0] - 1185.47).abs() < 0.01, "{side:?}");
        let modesty = after.iter().find(|b| b.name == "panel - front").unwrap();
        assert!((modesty.hi[0] - 1185.47).abs() < 0.01, "{modesty:?}");
    }

    /// The table case: a 1" side to 1/2", and a thicker board shortens its neighbour instead.
    #[test]
    fn thicker_shortens_and_a_doubled_panel_moves() {
        let mut boxes = desk();
        boxes[3] = b("side (2)", [1.59, -600.07, 7.01], [26.99, 0.0, 736.6]);
        boxes[1].lo[0] = 26.99;
        let (_, after) = plan(&boxes, "side (2)", 12.7).unwrap();
        assert!((after[1].lo[0] - 14.29).abs() < 0.01, "grows 12.7: {:?}", after[1]);
        let (_, back) = plan(&after, "side (2)", 25.4).unwrap();
        assert!((back[1].lo[0] - 26.99).abs() < 0.01, "shrinks back: {:?}", back[1]);
        // A panel glued face-to-face on the inside is moved, not stretched.
        let mut doubled = desk();
        doubled.push(b("liner", [20.64, -590.0, 100.0], [39.69, -10.0, 700.0]));
        let (plan, after) = plan(&doubled, "side (2)", 12.7).unwrap();
        assert!(plan.steps.iter().any(|s| matches!(s, Step::Shift { body, .. } if body == "liner")));
        let liner = after.iter().find(|b| b.name == "liner").unwrap();
        assert!((liner.lo[0] - 14.29).abs() < 0.01 && (liner.size(0) - 19.05).abs() < 1e-9);
    }

    /// A core inside an unmerged edgebanding ring is refused with the fix named.
    #[test]
    fn an_unmerged_band_is_refused() {
        let mut boxes = desk();
        boxes.push(b("Part (7)", [1.59, -600.07, 7.01], [20.64, 0.0, 736.6]));
        boxes[3] = b("side (2)", [1.59, -598.07, 9.01], [20.64, -2.0, 734.6]);
        let error = plan(&boxes, "side (2)", 12.7).unwrap_err();
        assert!(error.contains("Merge edgebanding"), "{error}");
    }

    #[test]
    fn the_same_thickness_changes_nothing() {
        let (plan, _) = plan(&desk(), "side (2)", 19.05).unwrap();
        assert!(plan.steps.is_empty());
    }
}
