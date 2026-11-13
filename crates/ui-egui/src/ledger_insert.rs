//! Hardware added by hand: a cam, a wooden dowel or a foot screw put onto the boards the operator picked, for the joint
//! the automatic placement missed or the extra one the shop wants.
//!
//! **Add ▾** in the Ledger tab's Hardware group. The fitting is drawn like the ones Calc Camlocks draws and recorded the
//! same way (`shop.ledger.fittings`, marked as changed by hand), so from then on it is ordinary hardware: move it with
//! the move tool (with **snap** on it slides along its joint), flip a cam, delete it with Delete - and **Calc Holes**
//! drills it where it stands, checking it like every other fitting.
//!
//! * **Cam / Wooden dowel** - select the two boards that meet. The bores are the engine's own for that joint (taken
//!   from what Calc Camlocks would place there, or from the engine run on just those two boards), so a cam added by
//!   hand is the same cam the rule draws: same housing, bolt, pilot and distances. It goes into the widest gap left
//!   on the joint, and is moved from there.
//! * **Foot screw** - select the board the foot sits under (and the foot, to put it over that foot). ⌀8 x 25 straight
//!   up into the board's underside.
//!
//! Nothing goes into a 1/2" board, by the owner's rule.

use std::collections::BTreeMap;

use ledger_holes::{Fastener, HoleKind, Panel};
use serde_json::json;

use crate::ledger_cams::{self, Box3};
use crate::ledger_fittings::Bore;

/// What can be added by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Add {
    Cam,
    WoodenDowel,
    FootScrew,
    Handle,
}

impl Add {
    pub const ALL: [Add; 4] = [Add::Cam, Add::WoodenDowel, Add::FootScrew, Add::Handle];

    pub fn label(self) -> &'static str {
        match self {
            Add::Cam => "Cam",
            Add::WoodenDowel => "Wooden dowel",
            Add::FootScrew => "Foot screw",
            Add::Handle => "Handle",
        }
    }

    fn fastener(self) -> Fastener {
        match self {
            Add::Cam => Fastener::Cam,
            Add::WoodenDowel => Fastener::Dowel,
            Add::FootScrew => Fastener::Screw,
            Add::Handle => Fastener::Handle,
        }
    }
}

/// Where two boards meet: one board's edge on the other's face.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Joint {
    /// The board whose edge meets the joint: it takes the bolt (and the housing).
    pub edge: String,
    /// The board whose face it meets: it takes the pilot.
    pub face: String,
    /// The world axis the joint runs along, and the stretch of it the two boards share.
    pub along: usize,
    pub from: f64,
    pub to: f64,
}

/// How close two faces must be to count as touching, mm.
const TOUCH_MM: f64 = 1.0;
/// The least the two boards must share along the joint, mm.
const SHARED_MM: f64 = 20.0;

fn thin_axis(b: &Box3) -> usize {
    (0..3).min_by(|x, y| (b.hi[*x] - b.lo[*x]).total_cmp(&(b.hi[*y] - b.lo[*y]))).unwrap_or(2)
}

fn thickness(b: &Box3) -> f64 {
    (0..3).map(|k| b.hi[k] - b.lo[k]).fold(f64::INFINITY, f64::min)
}

/// The joint between `a` and `b`, or why there is none a cam or dowel can go into.
pub(crate) fn joint_of(a: &Box3, b: &Box3) -> Result<Joint, String> {
    for k in 0..3 {
        let touching = (a.hi[k] - b.lo[k]).abs() < TOUCH_MM || (b.hi[k] - a.lo[k]).abs() < TOUCH_MM;
        if !touching {
            continue;
        }
        let others: Vec<usize> = (0..3).filter(|j| *j != k).collect();
        let shared = |j: usize| (a.hi[j].min(b.hi[j]), a.lo[j].max(b.lo[j]));
        if others.iter().any(|j| {
            let (hi, lo) = shared(*j);
            hi - lo <= 0.0
        }) {
            continue;
        }
        let (edge, face) = match (thin_axis(a) == k, thin_axis(b) == k) {
            (false, true) => (a, b),
            (true, false) => (b, a),
            (true, true) => {
                return Err(format!("{} and {} lie face to face - a cam or dowel joins one board's edge to the other's face", a.name, b.name));
            }
            (false, false) => {
                return Err(format!("{} and {} meet edge to edge - a cam or dowel joins one board's edge to the other's face", a.name, b.name));
            }
        };
        // Along the joint: the axis in the contact plane that is not the edge board's thickness.
        let edge_thin = thin_axis(edge);
        let along = others.iter().copied().find(|j| *j != edge_thin).unwrap_or(others[0]);
        let (to, from) = shared(along);
        if to - from < SHARED_MM {
            return Err(format!("{} and {} share only {:.0} mm - too little for hardware", a.name, b.name, to - from));
        }
        return Ok(Joint { edge: edge.name.clone(), face: face.name.clone(), along, from, to });
    }
    Err(format!("{} and {} do not touch", a.name, b.name))
}

/// The middle of the widest stretch of the joint between `taken` positions (and the joint's ends).
pub fn widest_gap(from: f64, to: f64, taken: &[f64]) -> f64 {
    let mut points: Vec<f64> = taken.iter().copied().filter(|x| *x > from && *x < to).collect();
    points.push(from);
    points.push(to);
    points.sort_by(f64::total_cmp);
    points
        .windows(2)
        .max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])))
        .map(|pair| (pair[0] + pair[1]) / 2.0)
        .unwrap_or((from + to) / 2.0)
}

/// Add one piece of hardware to the selected boards. One undo step. Answers the status line.
pub fn add(app: &mut crate::SolveApp, what: Add) -> Result<String, String> {
    crate::ledger_hand::as_one_step(app, "Add hardware", |app| add_inner(app, what))
}

fn add_inner(app: &mut crate::SolveApp, what: Add) -> Result<String, String> {
    if what == Add::Handle {
        return add_handle(app);
    }
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else {
        return Err("could not read the design".to_owned());
    };
    let bodies = ledger_cams::boxes(&measured);
    let chosen = crate::ledger_resize::selected(app);
    let picked = |want_feet: bool| -> Vec<&Box3> {
        bodies
            .iter()
            .filter(|b| chosen.contains(&b.name) && !b.is_marker() && ledger_cams::stays_solid(&b.name) == want_feet)
            .collect()
    };
    let boards = picked(false);
    let feet = picked(true);
    let panels = ledger_cams::panels_now(&bodies);
    let min = ledger_holes::rules::MIN_HARDWARE_THICKNESS;

    let (bores, edge, face, seam) = match what {
        Add::FootScrew => {
            let [board] = boards.as_slice() else {
                return Err("add foot screw: select the one board the foot sits under (and the foot, to put it over that foot)".to_owned());
            };
            if thickness(board) < min {
                return Err(format!("{} is 1/2\" board - no hardware goes into it", board.name));
            }
            let (cx, cy) = match feet.first() {
                Some(foot) => ((foot.lo[0] + foot.hi[0]) / 2.0, (foot.lo[1] + foot.hi[1]) / 2.0),
                None => ((board.lo[0] + board.hi[0]) / 2.0, (board.lo[1] + board.hi[1]) / 2.0),
            };
            let panel = panel_of(&panels, &board.name)?;
            let screw = ledger_cams::bore_at(panel, HoleKind::Screw, [cx, cy, board.lo[2]], [0.0, 0.0, 1.0], 8.0, 25.0, 0);
            (vec![screw], board.name.clone(), String::new(), [0.0; 3])
        }
        Add::Handle => return Err("a handle is added on its own".to_owned()),
        Add::Cam | Add::WoodenDowel => {
            let [a, b] = boards.as_slice() else {
                return Err(format!(
                    "add {}: select the two boards it joins (you have {} selected)",
                    what.label().to_lowercase(),
                    boards.len()
                ));
            };
            for board in [a, b] {
                if thickness(board) < min {
                    return Err(format!("{} is 1/2\" board - no hardware goes into it", board.name));
                }
            }
            let joint = joint_of(a, b)?;
            let template = template(&bodies, &joint, what)?;
            // Into the widest gap the joint has left, past the fittings already on it.
            let records = crate::ledger_hand::load(app);
            let now = crate::ledger_hand::boxes(app);
            let taken: Vec<f64> = records
                .iter()
                .filter_map(|(name, fitting)| {
                    let here = crate::ledger_hand::centre(*now.get(name)?);
                    let bores = crate::ledger_hand::bores_now(fitting, here);
                    let on = |board: &str| bores.iter().any(|b| b.board == board);
                    (on(&joint.edge) && on(&joint.face)).then(|| here[joint.along])
                })
                .collect();
            let anchor = template
                .iter()
                .find(|(kind, ..)| matches!(kind, HoleKind::EdgeBolt | HoleKind::DowelEdge))
                .map(|(_, mouth, ..)| mouth[joint.along])
                .ok_or("no bore up the joint to place it by")?;
            let target = widest_gap(joint.from, joint.to, &taken);
            let mut bores = Vec::new();
            for (kind, mouth, dir, dia, depth, board) in template {
                let mut mouth = mouth;
                mouth[joint.along] += target - anchor;
                bores.push(ledger_cams::bore_at(panel_of(&panels, &board)?, kind, mouth, dir, dia, depth, 0));
            }
            let mut seam = [0.0; 3];
            seam[joint.along] = 1.0;
            (bores, joint.edge, joint.face, seam)
        }
    };

    // Drawn like the rule's fittings, named past the ones already there.
    let refs: Vec<&Bore> = bores.iter().collect();
    let fastener = what.fastener();
    let word = match what {
        Add::Cam => "cam",
        Add::WoodenDowel => "wooden dowel",
        Add::FootScrew => "screw",
        Add::Handle => "handle",
    };
    let mut number = 1;
    while bodies.iter().any(|b| b.name == format!("{word} {number}")) {
        number += 1;
    }
    let name = format!("{word} {number}");
    let steps = crate::ledger_fittings::fitting_steps(fastener, &refs).ok_or("this fitting cannot be drawn")?;
    let body = crate::ledger_fittings::draw(app, &name, steps).ok_or("the fitting could not be drawn")?;
    let _ = app
        .session
        .execute("appearance.assign", &json!({ "bodies": [body.clone()], "color": crate::ledger_fittings::colour(fastener) }));
    let now = crate::ledger_hand::boxes(app);
    let at = now.get(&body).map(|b| crate::ledger_hand::centre(*b)).ok_or("the drawn fitting is not in the design")?;
    let frames = bores
        .iter()
        .map(|b| panels.iter().find(|p| p.name == b.board).map_or(([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), |p| (p.frame.x, p.frame.y)))
        .collect();
    let mut records = crate::ledger_hand::load(app);
    records.insert(
        body.clone(),
        crate::ledger_hand::Fitting {
            key: format!("hand|{edge}|{face}|{word}|{body}"),
            bores,
            at,
            seam,
            frames,
            flipped: false,
            hand: true,
        },
    );
    crate::ledger_hand::save(app, &records);
    ledger_cams::refile(app);
    app.session.selection = vec![solidcraft_engine::Sel::Body { name: body.clone() }];
    Ok(match what {
        Add::FootScrew => format!("{body} added under {edge} - move it if it is not where the foot is, then press Calc Holes"),
        _ => format!("{body} added on {edge} into {face} - move it along the joint if it is not where you want it, then press Calc Holes"),
    })
}

/// A bar handle on the one board picked, in the middle of its outside face; the operator moves it from there. It needs
/// no record: Calc Holes reads where the handle stands.
fn add_handle(app: &mut crate::SolveApp) -> Result<String, String> {
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else {
        return Err("could not read the design".to_owned());
    };
    let bodies = ledger_cams::boxes(&measured);
    let chosen = crate::ledger_resize::selected(app);
    let boards: Vec<&Box3> =
        bodies.iter().filter(|b| chosen.contains(&b.name) && !ledger_cams::stays_solid(&b.name)).collect();
    let [board] = boards.as_slice() else {
        return Err(format!("add handle: select the one board it goes on (you have {} selected)", boards.len()));
    };
    let mut piece = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for b in bodies.iter().filter(|b| !ledger_cams::stays_solid(&b.name)) {
        for k in 0..3 {
            piece.0[k] = piece.0[k].min(b.lo[k]);
            piece.1[k] = piece.1[k].max(b.hi[k]);
        }
    }
    let placed = crate::ledger_handle::place(board, piece)?;
    let mut number = 1;
    while bodies.iter().any(|b| b.name == format!("handle {number}")) {
        number += 1;
    }
    let name = format!("handle {number}");
    let body = crate::ledger_fittings::draw(app, &name, crate::ledger_handle::steps(&placed))
        .ok_or("the handle could not be drawn")?;
    let _ = app.session.execute(
        "appearance.assign",
        &json!({ "bodies": [body.clone()], "color": crate::ledger_fittings::colour(Fastener::Handle) }),
    );
    ledger_cams::refile(app);
    app.session.selection = vec![solidcraft_engine::Sel::Body { name: body.clone() }];
    Ok(format!("{body} added on {} - move it where it goes, then press Calc Holes for its bolt holes", board.name))
}

fn panel_of<'a>(panels: &'a [Panel], name: &str) -> Result<&'a Panel, String> {
    panels.iter().find(|p| p.name == name).ok_or_else(|| format!("{name} is not a board the hole rules can read"))
}

/// A template bore: kind, world mouth, drill direction, diameter, depth, board.
type Template = (HoleKind, [f64; 3], [f64; 3], f64, f64, String);

/// The bores the rule would give one fitting of this kind on this joint: from Calc Camlocks' own run over the whole
/// design, else from the engine run on just the two boards. A dowel the run did not place is made from a cam's bolt and
/// pilot, in the dowel the run uses elsewhere (or the wooden 8 x 10).
fn template(bodies: &[Box3], joint: &Joint, what: Add) -> Result<Vec<Template>, String> {
    let pair: Vec<Box3> = bodies
        .iter()
        .filter(|b| b.name == joint.edge || b.name == joint.face)
        .map(|b| Box3 { name: b.name.clone(), lo: b.lo, hi: b.hi })
        .collect();
    let runs = [ledger_cams::run_on(bodies), ledger_cams::run_on(&pair)];
    let mut dowel_size: Option<(f64, f64, f64)> = None;
    let mut cam: Option<Vec<Template>> = None;
    for (panels, run) in runs.iter().flatten() {
        let mut stations: BTreeMap<u32, Vec<Template>> = BTreeMap::new();
        for hole in &run.holes {
            let Some(panel) = panels.iter().find(|p| p.id == hole.part_id) else { continue };
            let bore = crate::ledger_fittings::bore(panel, hole);
            stations.entry(hole.station).or_default().push((hole.kind, bore.mouth, bore.dir, bore.dia, bore.depth, bore.board));
        }
        for group in stations.values() {
            let size = |kind: HoleKind| group.iter().find(|t| t.0 == kind).map(|t| (t.3, t.4));
            if let (Some((dia, edge)), Some((_, mate))) = (size(HoleKind::DowelEdge), size(HoleKind::DowelMateHole)) {
                dowel_size.get_or_insert((dia, edge, mate));
            }
            let on_joint = group.iter().all(|t| t.5 == joint.edge || t.5 == joint.face)
                && group.iter().any(|t| t.5 == joint.edge && matches!(t.0, HoleKind::EdgeBolt | HoleKind::DowelEdge));
            if !on_joint {
                continue;
            }
            let has = |kind: HoleKind| group.iter().any(|t| t.0 == kind);
            match what {
                Add::WoodenDowel if has(HoleKind::DowelEdge) => return Ok(group.clone()),
                Add::Cam if has(HoleKind::EdgeBolt) && has(HoleKind::CamCup) => return Ok(group.clone()),
                _ if has(HoleKind::EdgeBolt) && cam.is_none() => cam = Some(group.clone()),
                _ => {}
            }
        }
    }
    let Some(cam) = cam else {
        return Err(format!(
            "the hole rules do not join {} to {}, so there is no cam or dowel position to start from - check the two boards really meet edge to face",
            joint.edge, joint.face
        ));
    };
    if what == Add::Cam {
        return Ok(cam);
    }
    // A dowel where the run put only cams: the cam's bolt and pilot, in the dowel's own size.
    let wooden = ledger_holes::table_rules::DowelHardware::WOODEN_8X10;
    let (dia, edge_depth, mate_depth) = dowel_size.unwrap_or((wooden.dia, wooden.edge_depth, wooden.mate_depth));
    Ok(cam
        .into_iter()
        .filter_map(|(kind, mouth, dir, _, _, board)| match kind {
            HoleKind::EdgeBolt => Some((HoleKind::DowelEdge, mouth, dir, dia, edge_depth, board)),
            HoleKind::MatePreHole => Some((HoleKind::DowelMateHole, mouth, dir, dia, mate_depth, board)),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, lo: [f64; 3], hi: [f64; 3]) -> Box3 {
        Box3 { name: name.into(), lo, hi }
    }

    /// The desk: a side standing under the top meets it edge to face; the joint runs front to back along y.
    #[test]
    fn a_side_under_the_top_is_its_edge_on_the_tops_face() {
        let side = b("side", [0.0, -600.0, 7.0], [19.05, 0.0, 736.6]);
        let top = b("top", [0.0, -600.0, 736.6], [1200.0, 0.0, 762.0]);
        let joint = joint_of(&side, &top).expect("a joint");
        assert_eq!((joint.edge.as_str(), joint.face.as_str(), joint.along), ("side", "top", 1));
        assert!((joint.from + 600.0).abs() < 1e-9 && joint.to.abs() < 1e-9);
        // Either order picks the same boards.
        assert_eq!(joint_of(&top, &side).expect("a joint").edge, "side");
    }

    #[test]
    fn boards_that_do_not_meet_edge_to_face_are_refused() {
        let a = b("a", [0.0, 0.0, 0.0], [600.0, 400.0, 19.05]);
        let stacked = b("b", [0.0, 0.0, 19.05], [600.0, 400.0, 38.1]);
        assert!(joint_of(&a, &stacked).unwrap_err().contains("face to face"));
        let apart = b("c", [0.0, 0.0, 100.0], [600.0, 400.0, 119.05]);
        assert!(joint_of(&a, &apart).unwrap_err().contains("do not touch"));
    }

    #[test]
    fn a_new_fitting_goes_in_the_widest_gap() {
        assert_eq!(widest_gap(0.0, 600.0, &[]), 300.0);
        assert_eq!(widest_gap(0.0, 600.0, &[50.8, 549.2]), 300.0);
        // Cams at 50.8 and 200: the widest stretch is 200..600.
        assert_eq!(widest_gap(0.0, 600.0, &[50.8, 200.0]), 400.0);
    }
}
