//! What the hole engine's answer looks like in the model: each bore as a world point and a drill direction, the fitting
//! that goes in it drawn as the part it is (a cam housing turned to face its bolt, a dowel across the seam, a shelf pin),
//! and the bores themselves cut into the boards by **Calc Holes**.
//!
//! # A bore in the world
//!
//! The engine reports a hole in its panel's frame on the vendor's scales ([`ledger_holes::Hole::at`]); the mouth is
//! [`ledger_holes::hole_at_world`]. The drill direction is not stored at all, because the vendor never needed it: a
//! face hole goes in through the face it is on, an edge hole in through the edge it is on. So the direction is read off
//! where the mouth sits - on the `±z` face for a face hole, on whichever of the `±x`/`±y` edges it touches for an edge
//! hole - pointing back toward the panel's centre. A test drills every bore of a real run and checks each one goes
//! *into* its own board.
//!
//! # The fittings, turned the way they are fitted
//!
//! A camlock is two parts that meet inside the board carrying it: the ⌀15 housing sunk in the red face 33 mm from the
//! edge, and the bolt coming in through the ⌀8 edge bore from the mate. The housing has an opening on one side for the
//! bolt's head; fitted, that opening faces the edge the bolt comes from - which is the "rotation" that matters. So the
//! housing is a ⌀15 x 13.5 cylinder down the cup's axis with a notch cut on the bolt's side and a slot across its top
//! pointing along the bolt, and the bolt runs from 10 mm inside the mate (the ⌀10 pre-hole) to the housing. Both are
//! read from the station's own two bores, so nothing about the orientation is assumed.
//!
//! These are drawings of the hardware, not the supplier's model: sizes come from the bores the engine drills (⌀15 cup,
//! ⌀8 bolt bore, ⌀10 pre-hole, ⌀8 x 40 dowel, ⌀5 pin), not from a spec sheet.
//!
//! # Holes
//!
//! Calc Holes draws every cam and dowel bore (and shelf pin) as a dark cylinder of the bore's own diameter and depth,
//! standing in the board from the face it is drilled from (`hole N`, filed under `Hardware › Holes`), and records the
//! list in the design (`shop.ledger.bores`) in each board's own frame - measured from the board's corner, with the
//! face it is drilled from - which is what a machine file is written from.
//!
//! The bores are **not** cut into the boards. Cutting them is the kernel's slowest operation: the SOL-DKS desk's 33
//! bores took 23-25 s in the bare engine (17 s for the modesty alone, where the bolt bore runs into the housing) and
//! 81 s in the editor, which cannot answer meanwhile - on the owner's own design it looked like a crash. A marker
//! takes about 2 ms, leaves the boards whole for the audit, the edgebanding and the grain, and shows the same thing.
//! An earlier build's cut holes (features `ledger bore N`) are deleted on the next press.

use ledger_holes::{Face, Fastener, Hole, HoleKind, Panel, hole_at_world};
use serde_json::{Value, json};

/// The name every hole feature this pass cuts starts with: how a second press finds the first one's holes.
pub const BORE_FEATURE: &str = "ledger bore ";

/// One bore, where it really is.
#[derive(Debug, Clone)]
pub struct Bore {
    /// The board (body) it is drilled into.
    pub board: String,
    pub kind: HoleKind,
    pub station: u32,
    pub face: Face,
    /// The mouth, in model millimetres.
    pub mouth: [f64; 3],
    /// Unit vector the drill travels along, into the board.
    pub dir: [f64; 3],
    pub dia: f64,
    pub depth: f64,
    /// The mouth in the board's own frame, measured from its corner: `x` along its width, `y` along its height (as seen
    /// from the red face, not mirrored for a back hole), `z` from the near face.
    pub local: [f64; 3],
    /// The board's width, height and thickness: what the machine file needs beside `local`.
    pub panel: [f64; 3],
}

impl Bore {
    pub fn fastener(&self) -> Fastener {
        self.kind.fastener()
    }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// A bore of the engine's answer, placed in the world and given its drill direction.
pub fn bore(panel: &Panel, hole: &Hole) -> Bore {
    let mouth = hole_at_world(panel, hole);
    let y = if hole.face == Face::Back { -hole.at[1] } else { hole.at[1] };
    let local = [hole.at[0], y, hole.at[2] - panel.thickness / 2.0];
    let half = [panel.width / 2.0, panel.height / 2.0, panel.thickness / 2.0];
    // The face the drill enters from: the thickness for a face hole; for an edge hole, whichever edge the mouth is on.
    let axis = match hole.face {
        Face::Edge => {
            if local[0].abs() / half[0].max(1e-9) >= local[1].abs() / half[1].max(1e-9) {
                0
            } else {
                1
            }
        }
        Face::Front | Face::Back => 2,
    };
    let unit = [panel.frame.x, panel.frame.y, panel.frame.z][axis];
    // Into the board: back toward its centre from the face the mouth is on.
    let dir = if local[axis] > 0.0 { scale(unit, -1.0) } else { unit };
    Bore {
        board: panel.name.clone(),
        kind: hole.kind,
        station: hole.station,
        face: hole.face,
        mouth,
        dir,
        dia: hole.dia,
        depth: hole.depth,
        local: [local[0] + half[0], local[1] + half[1], hole.at[2]],
        panel: [panel.width, panel.height, panel.thickness],
    }
}

/// An axis-aligned box's centre and size from a centre and three extents along three (axis-aligned) directions.
fn box_params(centre: [f64; 3], along: [([f64; 3], f64); 3]) -> Value {
    let mut size = [0.0; 3];
    for (dir, extent) in along {
        for (axis, value) in size.iter_mut().enumerate() {
            *value += dir[axis].abs() * extent;
        }
    }
    json!({ "center": centre, "length": size[0], "width": size[1], "height": size[2] })
}

/// One command of a fitting's drawing. The first makes the body; the rest name it in `targets`, filled in once the
/// kernel has said what the body is actually called.
#[derive(Debug, Clone)]
pub struct Step {
    pub command: &'static str,
    pub params: Value,
}

/// The cam housing's height: as deep as it sits in the cup, just short of the ⌀15 x 14.2 bore.
const HOUSING_TALL: f64 = 13.5;

/// The commands that draw one fitting, from its station's bores. `None` when the station lacks the bore the drawing
/// is placed from (a cam whose cup or bolt the engine dropped); the caller then draws the plain block.
pub fn fitting_steps(fastener: Fastener, bores: &[&Bore]) -> Option<Vec<Step>> {
    let find = |kind: HoleKind| bores.iter().find(|bore| bore.kind == kind).copied();
    match fastener {
        Fastener::Cam => {
            let cup = find(HoleKind::CamCup)?;
            let bolt = find(HoleKind::EdgeBolt)?;
            let n = cup.dir;
            // The housing opens toward its bolt: from the cup's axis toward where the bolt comes out of its edge, in the
            // plane of the face the cup is sunk in. In the vendor's joint that is back toward the source's edge, 33 mm
            // away; in the shop's desk standard (cup in the receiving board) it is toward the seam, 33 mm away on the
            // receiving board. Either way the turn is read off the two bores, never assumed.
            let toward = {
                let raw = sub(bolt.mouth, cup.mouth);
                let flat = sub(raw, scale(n, dot(raw, n)));
                let flat = if dot(flat, flat) < 1.0 { sub(scale(bolt.dir, -1.0), scale(n, dot(scale(bolt.dir, -1.0), n))) } else { flat };
                let len = dot(flat, flat).sqrt();
                if len < 1e-6 {
                    return None;
                }
                scale(flat, 1.0 / len)
            };
            let t = cross(n, toward);
            // The bolt's line under the face, where it crosses the housing: mid-board in the vendor's joint, and the
            // housing's middle when the bolt stops at the face (the desk standard).
            let deep = dot(sub(bolt.mouth, cup.mouth), n).clamp(4.0, HOUSING_TALL - 3.5);
            let centre = add(cup.mouth, scale(n, deep));
            let mut steps = vec![Step {
                command: "solid.cylinder",
                params: json!({ "base": cup.mouth, "axis": n, "diameter": 15.0, "height": HOUSING_TALL }),
            }];
            // The opening for the bolt's head, on the bolt's side: what the housing is turned by.
            let mut notch = box_params(add(centre, scale(toward, 5.0)), [(toward, 10.0), (n, 7.0), (t, 8.0)]);
            notch["operation"] = json!("cut");
            steps.push(Step { command: "solid.box", params: notch });
            // The driver slot across its top, pointing at the bolt, so the turn reads from outside too.
            let mut slot = box_params(add(cup.mouth, scale(n, 0.75)), [(toward, 12.0), (n, 1.5), (t, 1.8)]);
            slot["operation"] = json!("cut");
            steps.push(Step { command: "solid.box", params: slot });
            let b = bolt.dir;
            if cup.board == bolt.board {
                // The vendor's joint: threaded 10 mm into the mate's ⌀10, up the ⌀8 edge bore, its head in the housing.
                let reach = dot(sub(centre, bolt.mouth), b);
                steps.push(Step {
                    command: "solid.cylinder",
                    params: json!({ "base": add(bolt.mouth, scale(b, -10.0)), "axis": b, "diameter": 6.0,
                        "height": (reach + 10.0 - 1.5).max(12.0), "operation": "join" }),
                });
                steps.push(Step {
                    command: "solid.cylinder",
                    params: json!({ "base": add(centre, scale(b, -4.5)), "axis": b, "diameter": 7.4, "height": 3.5,
                        "operation": "join" }),
                });
            } else {
                // The desk standard: the bolt sits in the source's ⌀8 x 33 edge bore and stands out into the receiving
                // board's ⌀10 x 11, its head in that pre-hole.
                steps.push(Step {
                    command: "solid.cylinder",
                    params: json!({ "base": add(bolt.mouth, scale(b, (bolt.depth - 1.0).max(10.0))), "axis": scale(b, -1.0),
                        "diameter": 6.0, "height": (bolt.depth - 1.0).max(10.0) + 6.0, "operation": "join" }),
                });
                steps.push(Step {
                    command: "solid.cylinder",
                    params: json!({ "base": add(bolt.mouth, scale(b, -5.5)), "axis": scale(b, -1.0), "diameter": 7.4,
                        "height": 3.5, "operation": "join" }),
                });
            }
            Some(steps)
        }
        Fastener::Dowel => {
            // The source's edge bore says which way the dowel lies; a mate-only station is turned around.
            let (mouth, dir) = match find(HoleKind::DowelEdge) {
                Some(edge) => (edge.mouth, edge.dir),
                None => {
                    let mate = find(HoleKind::DowelMateHole)?;
                    (mate.mouth, scale(mate.dir, -1.0))
                }
            };
            Some(vec![Step {
                command: "solid.cylinder",
                params: json!({ "base": add(mouth, scale(dir, -10.0)), "axis": dir, "diameter": 8.0, "height": 40.0 }),
            }])
        }
        Fastener::ShelfPin => {
            let pin = find(HoleKind::ShelfPin)?;
            // In its ⌀5 bore with 6 mm standing proud of the face for the shelf to sit on.
            Some(vec![Step {
                command: "solid.cylinder",
                params: json!({ "base": add(pin.mouth, scale(pin.dir, 10.0)), "axis": scale(pin.dir, -1.0),
                    "diameter": 5.0, "height": 16.0 }),
            }])
        }
    }
}

/// The colour a fitting is drawn in: zinc for a cam, beech for a dowel, steel for a pin.
pub fn colour(fastener: Fastener) -> &'static str {
    match fastener {
        Fastener::Cam => "#A9AEB4",
        Fastener::Dowel => "#C9A66B",
        Fastener::ShelfPin => "#8E9399",
    }
}

/// Draw one fitting under `name`. Answers the body's name as the tree shows it, or `None` when nothing was made.
pub fn draw(app: &mut crate::SolveApp, name: &str, steps: Vec<Step>) -> Option<String> {
    let mut steps = steps.into_iter();
    let first = steps.next()?;
    let mut params = first.params;
    params["name"] = json!(name);
    params["body_name"] = json!(name);
    let made = app.session.execute(first.command, &params).ok()?;
    let body = made
        .get("created_bodies")
        .and_then(Value::as_array)
        .and_then(|list| list.first())
        .and_then(Value::as_str)
        .map_or_else(|| name.to_owned(), str::to_owned);
    for step in steps {
        let mut params = step.params;
        params["targets"] = json!([body]);
        // A cut that misses (a housing sunk deeper than its notch) leaves the part plainer, never missing.
        let _ = app.session.execute(step.command, &params);
    }
    Some(body)
}

/// What Calc Holes did.
#[derive(Debug, Default, Clone)]
pub struct HolesOutcome {
    pub drawn: usize,
    pub refused: usize,
    pub boards: usize,
    /// Holes an earlier build cut into the boards, taken out again.
    pub uncut: usize,
    pub by_size: Vec<(f64, f64, usize)>,
    /// The engine's own message when there was nothing to drill.
    pub nothing: Option<String>,
    /// The names of the markers drawn, for the tree.
    pub names: Vec<String>,
}

impl HolesOutcome {
    pub fn message(&self) -> String {
        if let Some(why) = &self.nothing {
            return why.clone();
        }
        let sizes: Vec<String> =
            self.by_size.iter().map(|(dia, depth, n)| format!("{n} at {dia} mm x {depth} deep")).collect();
        let mut line = format!(
            "{} {} in {} {} ({})",
            self.drawn,
            if self.drawn == 1 { "hole" } else { "holes" },
            self.boards,
            if self.boards == 1 { "board" } else { "boards" },
            sizes.join(", ")
        );
        if self.uncut > 0 {
            line.push_str(&format!("; {} holes cut by an earlier build filled back in", self.uncut));
        }
        if self.refused > 0 {
            line.push_str(&format!("; {} could not be drawn", self.refused));
        }
        line.push_str("; hole list saved in the design for the machine file");
        line
    }
}

/// The name a hole marker is drawn under.
pub const HOLE_MARKER: &str = "hole ";

/// The hole features an earlier build's Calc Holes cut, by id.
pub fn earlier_bores(app: &crate::SolveApp) -> Vec<u64> {
    app.session.doc.features.iter().filter(|feature| feature.name.starts_with(BORE_FEATURE)).map(|f| f.id).collect()
}

/// Delete the holes an earlier build cut into the boards, so they are whole again. Answers how many.
pub fn clear_bores(app: &mut crate::SolveApp) -> usize {
    let ids = earlier_bores(app);
    if ids.is_empty() {
        return 0;
    }
    let items: Vec<Value> = ids.iter().map(|id| json!({ "type": "feature", "id": id })).collect();
    match app.session.execute("selection.delete", &json!({ "items": items })) {
        Ok(_) => ids.len(),
        Err(_) => 0,
    }
}

/// Draw every bore as a marker, and keep the list for the machine file. `bores` come from one engine run.
pub fn mark(app: &mut crate::SolveApp, bores: &[Bore]) -> HolesOutcome {
    let mut outcome = HolesOutcome::default();
    let mut boards = std::collections::BTreeSet::new();
    let mut sizes: std::collections::BTreeMap<(i64, i64), usize> = std::collections::BTreeMap::new();
    let mut record: Vec<Value> = Vec::new();
    for (index, bore) in bores.iter().enumerate() {
        let name = format!("{HOLE_MARKER}{}", index + 1);
        // A hair proud of the face, so the marker reads as the bore's mouth rather than vanishing into the board.
        let made = app.session.execute(
            "solid.cylinder",
            &json!({
                "base": add(bore.mouth, scale(bore.dir, -0.3)),
                "axis": bore.dir,
                "diameter": bore.dia,
                "height": bore.depth + 0.3,
                "name": name,
                "body_name": name,
            }),
        );
        let Ok(made) = made else {
            outcome.refused += 1;
            continue;
        };
        let body = made
            .get("created_bodies")
            .and_then(Value::as_array)
            .and_then(|list| list.first())
            .and_then(Value::as_str)
            .map_or_else(|| name.clone(), str::to_owned);
        outcome.names.push(body);
        outcome.drawn += 1;
        boards.insert(bore.board.clone());
        *sizes.entry(((bore.dia * 10.0).round() as i64, (bore.depth * 10.0).round() as i64)).or_default() += 1;
        record.push(json!({
            "board": bore.board,
            "fitting": match bore.fastener() { Fastener::Cam => "cam", Fastener::Dowel => "dowel", Fastener::ShelfPin => "pin" },
            "bore": format!("{:?}", bore.kind),
            "station": bore.station,
            "face": match bore.face { Face::Front => "front", Face::Back => "back", Face::Edge => "edge" },
            "x": round1(bore.local[0]),
            "y": round1(bore.local[1]),
            "z": round1(bore.local[2]),
            "diameter": bore.dia,
            "depth": bore.depth,
            "board_size": [round1(bore.panel[0]), round1(bore.panel[1]), round1(bore.panel[2])],
            "mouth": [round1(bore.mouth[0]), round1(bore.mouth[1]), round1(bore.mouth[2])],
            "direction": bore.dir,
        }));
    }
    if !outcome.names.is_empty() {
        let _ = app.session.execute("appearance.assign", &json!({ "bodies": outcome.names, "color": "#1E1E1E" }));
    }
    outcome.boards = boards.len();
    outcome.by_size = sizes.into_iter().map(|((dia, depth), n)| (dia as f64 / 10.0, depth as f64 / 10.0, n)).collect();
    crate::ledger_boards::set_ledger_entry(
        app,
        "bores",
        json!({ "format": "ledger-bores/1", "units": "mm", "holes": record }),
    );
    outcome
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_holes::translate::{Board, translate_boards};
    use ledger_holes::{Settings, plan};

    /// SOL-DKS-2448 as it measures today: top, two sides, modesty.
    fn desk() -> Vec<Board> {
        let board = |name: &str, lo: [f64; 3], hi: [f64; 3]| Board {
            name: name.to_owned(),
            lo,
            hi,
            role: None,
            declared_role: None,
            link: None,
            hole_txt: None,
        };
        vec![
            board("top", [0.0, -600.1, 736.6], [1200.1, 0.0, 762.0]),
            board("panel - front", [20.6, -590.6, 7.0], [1179.1, -571.5, 736.6]),
            board("side", [1179.1, -600.1, 7.0], [1198.2, 0.0, 736.6]),
            board("side (2)", [1.6, -600.1, 7.0], [20.6, 0.0, 736.6]),
        ]
    }

    fn bores() -> (Vec<Board>, Vec<Bore>) {
        let boards = desk();
        let mut panels = translate_boards(&boards).panels;
        let run = plan(&mut panels, &Settings::default());
        let bores = run
            .holes
            .iter()
            .filter_map(|hole| panels.iter().find(|p| p.id == hole.part_id).map(|panel| bore(panel, hole)))
            .collect();
        (boards, bores)
    }

    /// Every bore goes *into* its own board: its mouth is on the board's surface, and a step along the drill is inside.
    #[test]
    fn every_bore_drills_into_its_own_board() {
        let (boards, bores) = bores();
        assert!(!bores.is_empty());
        for bore in &bores {
            let board = boards.iter().find(|b| b.name == bore.board).expect("board");
            let inside = add(bore.mouth, scale(bore.dir, bore.depth.min(5.0)));
            for axis in 0..3 {
                assert!(
                    inside[axis] > board.lo[axis] - 0.05 && inside[axis] < board.hi[axis] + 0.05,
                    "{:?} in {} leaves the board on axis {axis}: {:?} -> {:?}",
                    bore.kind,
                    bore.board,
                    bore.mouth,
                    bore.dir
                );
            }
            let on_surface = (0..3).any(|axis| {
                (bore.mouth[axis] - board.lo[axis]).abs() < 0.05 || (bore.mouth[axis] - board.hi[axis]).abs() < 0.05
            });
            assert!(on_surface, "{:?} mouth is not on {}'s surface: {:?}", bore.kind, bore.board, bore.mouth);
        }
    }

    /// A cam housing opens toward the edge its bolt comes in from, and the bolt reaches it.
    #[test]
    fn a_cam_faces_its_bolt() {
        let (_, bores) = bores();
        let mut checked = 0;
        for station in bores.iter().map(|b| b.station).collect::<std::collections::BTreeSet<_>>() {
            let group: Vec<&Bore> = bores.iter().filter(|b| b.station == station).collect();
            let (Some(cup), Some(bolt)) = (
                group.iter().find(|b| b.kind == HoleKind::CamCup),
                group.iter().find(|b| b.kind == HoleKind::EdgeBolt),
            ) else {
                continue;
            };
            // The cup sits 33 mm from the bolt's mouth, whichever board it is in.
            let gap = sub(cup.mouth, bolt.mouth);
            let flat = sub(gap, scale(cup.dir, dot(gap, cup.dir)));
            assert!((dot(flat, flat).sqrt() - 33.0).abs() < 1.0, "cup {:.1} mm from the bolt, not 33", dot(flat, flat).sqrt());
            let steps = fitting_steps(Fastener::Cam, &group).expect("a cam is drawn");
            // The notch is centred on the bolt's side of the housing's axis.
            let notch = &steps[1].params["center"];
            let notch = [notch[0].as_f64().unwrap(), notch[1].as_f64().unwrap(), notch[2].as_f64().unwrap()];
            assert!(dot(sub(notch, cup.mouth), flat) < 0.0, "the opening faces away from the bolt");
            checked += 1;
        }
        assert!(checked > 0, "the desk has cams");
    }

    /// The shop's desk standard puts the cup in the receiving board: the housing still opens toward the bolt.
    #[test]
    fn a_desk_cam_in_the_receiving_board_faces_its_bolt() {
        let cup = Bore {
            board: "top".into(),
            kind: HoleKind::CamCup,
            station: 8,
            face: Face::Front,
            mouth: [1155.6, -400.1, 736.6],
            dir: [0.0, 0.0, 1.0],
            dia: 15.0,
            depth: 14.2,
            local: [0.0; 3],
            panel: [0.0; 3],
        };
        let bolt = Bore { board: "side".into(), kind: HoleKind::EdgeBolt, face: Face::Edge, mouth: [1188.1, -400.1, 736.6],
            dir: [0.0, 0.0, -1.0], dia: 8.0, depth: 33.0, ..cup.clone() };
        let steps = fitting_steps(Fastener::Cam, &[&cup, &bolt]).expect("drawn");
        let notch = &steps[1].params["center"];
        assert!(notch[0].as_f64().unwrap() > 1155.6, "the opening faces the seam at x = 1188: {notch}");
        // The bolt stands in the side's edge bore and out into the top.
        let shank = &steps[3].params;
        assert_eq!(shank["axis"], json!([-0.0, -0.0, 1.0]));
    }

    #[test]
    fn a_dowel_spans_the_seam() {
        let edge = Bore {
            board: "side".into(),
            kind: HoleKind::DowelEdge,
            station: 1,
            face: Face::Edge,
            mouth: [0.0, 0.0, 100.0],
            dir: [0.0, 0.0, -1.0],
            dia: 8.0,
            depth: 40.0,
            local: [0.0; 3],
            panel: [0.0; 3],
        };
        let steps = fitting_steps(Fastener::Dowel, &[&edge]).unwrap();
        assert_eq!(steps[0].params["base"], json!([0.0, 0.0, 110.0]));
        assert_eq!(steps[0].params["height"], json!(40.0));
    }
}
