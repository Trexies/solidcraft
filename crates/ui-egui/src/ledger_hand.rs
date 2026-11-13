//! Hardware moved and flipped by hand, and the holes that follow it.
//!
//! **Calc Camlocks** places every cam and dowel by the shop's rule and writes down, for each fitting it draws, which
//! bores belong to it, where the fitting stood when they were worked out, and the line of the joint it holds
//! (`shop.ledger.fittings`). From then on the fitting is the authority:
//!
//! * the operator moves it with SolidCraft's own move tool - freely, or, while **snap** is on, only along its joint (a
//!   move off the seam is put back onto it, [`snap_moved`]);
//! * **Flip cam** turns a cam over so it sits in the other face of the same board ([`flip`]); nothing is drilled
//!   until Calc Holes, which then puts the ⌀15 housing where the cam now is;
//! * **Calc Holes** drills each fitting where it is *now*: its bores moved by the same amount it was moved, the
//!   housing on the face it was flipped to ([`bores_now`]), and every bore checked ([`check`]) - still on its joint,
//!   inside its board, clear of every other hole, not in 1/2" board. A failed check is named in amber and no machine
//!   list is saved until it is fixed.
//!
//! A fitting that was moved or flipped is **changed by hand**; pressing Calc Camlocks again asks whether to keep those
//! and re-place only the rest, or to put everything back by the rule.

use serde_json::{Map, Value, json};

use ledger_holes::{Face, HoleKind};

use crate::ledger_fittings::Bore;

/// Where the record lives under `shop.ledger`.
pub const KEY: &str = "fittings";

/// A move smaller than this, in mm, is not a move: a re-measured box differs by hundredths.
const MOVED_MM: f64 = 0.05;
/// How close to a board's edge or face a bore's wall may come, in mm.
const RIM_MM: f64 = 3.0;
/// Board left under the bottom of a blind bore, in mm.
const FLOOR_MM: f64 = 0.5;

/// One fitting as Calc Camlocks left it.
#[derive(Debug, Clone)]
pub struct Fitting {
    /// Which fitting of the rule's layout this is - `source|mate|word|n` - so a second Calc Camlocks can tell the cam it
    /// would place from the one already standing there.
    pub key: String,
    /// The fitting's bores where they were when `at` was measured.
    pub bores: Vec<Bore>,
    /// The fitting body's box centre then.
    pub at: [f64; 3],
    /// Along the joint, a unit vector; zero for a fitting with no joint line (a shelf pin).
    pub seam: [f64; 3],
    /// Each bore's board frame's `x` and `y` in the world, for the machine list's board-frame positions.
    pub frames: Vec<([f64; 3], [f64; 3])>,
    pub flipped: bool,
    /// Moved or flipped by hand since Calc Camlocks placed it.
    pub hand: bool,
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
fn len(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn axis_of(dir: [f64; 3]) -> usize {
    (0..3).max_by(|a, b| dir[*a].abs().total_cmp(&dir[*b].abs())).unwrap_or(2)
}

fn kind_name(kind: HoleKind) -> String {
    format!("{kind:?}")
}

fn kind_from(name: &str) -> Option<HoleKind> {
    Some(match name {
        "EdgeBolt" => HoleKind::EdgeBolt,
        "CamCup" => HoleKind::CamCup,
        "MatePreHole" => HoleKind::MatePreHole,
        "DowelEdge" => HoleKind::DowelEdge,
        "DowelMateHole" => HoleKind::DowelMateHole,
        "ShelfPin" => HoleKind::ShelfPin,
        "Screw" => HoleKind::Screw,
        _ => return None,
    })
}

fn face_name(face: Face) -> &'static str {
    match face {
        Face::Front => "front",
        Face::Back => "back",
        Face::Edge => "edge",
    }
}

fn face_from(name: &str) -> Face {
    match name {
        "front" => Face::Front,
        "back" => Face::Back,
        _ => Face::Edge,
    }
}

fn v3(value: Option<&Value>) -> Option<[f64; 3]> {
    let list = value?.as_array()?;
    Some([list.first()?.as_f64()?, list.get(1)?.as_f64()?, list.get(2)?.as_f64()?])
}

fn bore_json(bore: &Bore, frame: ([f64; 3], [f64; 3])) -> Value {
    json!({
        "board": bore.board, "kind": kind_name(bore.kind), "station": bore.station, "face": face_name(bore.face),
        "mouth": bore.mouth, "dir": bore.dir, "dia": bore.dia, "depth": bore.depth, "local": bore.local,
        "panel": bore.panel, "fx": frame.0, "fy": frame.1,
    })
}

fn bore_from(value: &Value) -> Option<(Bore, ([f64; 3], [f64; 3]))> {
    let bore = Bore {
        board: value.get("board")?.as_str()?.to_owned(),
        kind: kind_from(value.get("kind")?.as_str()?)?,
        station: u32::try_from(value.get("station")?.as_u64()?).ok()?,
        face: face_from(value.get("face")?.as_str()?),
        mouth: v3(value.get("mouth"))?,
        dir: v3(value.get("dir"))?,
        dia: value.get("dia")?.as_f64()?,
        depth: value.get("depth")?.as_f64()?,
        local: v3(value.get("local"))?,
        panel: v3(value.get("panel"))?,
    };
    let frame = (v3(value.get("fx")).unwrap_or([1.0, 0.0, 0.0]), v3(value.get("fy")).unwrap_or([0.0, 1.0, 0.0]));
    Some((bore, frame))
}

impl Fitting {
    fn to_json(&self) -> Value {
        json!({
            "key": self.key,
            "at": self.at,
            "seam": self.seam,
            "flipped": self.flipped,
            "hand": self.hand,
            "bores": self.bores.iter().zip(&self.frames).map(|(b, f)| bore_json(b, *f)).collect::<Vec<_>>(),
        })
    }

    fn from_json(value: &Value) -> Option<Fitting> {
        let mut bores = Vec::new();
        let mut frames = Vec::new();
        for item in value.get("bores")?.as_array()? {
            let (bore, frame) = bore_from(item)?;
            bores.push(bore);
            frames.push(frame);
        }
        Some(Fitting {
            key: value.get("key")?.as_str()?.to_owned(),
            bores,
            at: v3(value.get("at"))?,
            seam: v3(value.get("seam")).unwrap_or([0.0; 3]),
            frames,
            flipped: value.get("flipped").and_then(Value::as_bool).unwrap_or(false),
            hand: value.get("hand").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// Whether the fitting body now stands somewhere other than where its bores were worked out.
    pub fn moved(&self, centre: [f64; 3]) -> bool {
        len(sub(centre, self.at)) > MOVED_MM
    }
}

/// Every fitting the design records, by body name.
pub fn load(app: &mut crate::SolveApp) -> std::collections::BTreeMap<String, Fitting> {
    let meta = crate::ledger_boards::ledger_meta(app);
    let mut out = std::collections::BTreeMap::new();
    if let Some(map) = meta.get(KEY).and_then(Value::as_object) {
        for (name, value) in map {
            if let Some(fitting) = Fitting::from_json(value) {
                out.insert(name.clone(), fitting);
            }
        }
    }
    out
}

/// Write the record whole.
pub fn save(app: &mut crate::SolveApp, fittings: &std::collections::BTreeMap<String, Fitting>) {
    let map: Map<String, Value> = fittings.iter().map(|(name, f)| (name.clone(), f.to_json())).collect();
    crate::ledger_boards::set_ledger_entry(app, KEY, Value::Object(map));
}

/// Every body's box, by name.
pub fn boxes(app: &mut crate::SolveApp) -> std::collections::BTreeMap<String, ([f64; 3], [f64; 3])> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else { return out };
    for body in measured.get("bodies").and_then(Value::as_array).into_iter().flatten() {
        let (Some(name), Some(bbox)) = (body.get("name").and_then(Value::as_str), body.get("bbox")) else { continue };
        if let (Some(lo), Some(hi)) = (v3(bbox.get("min")), v3(bbox.get("max"))) {
            out.insert(name.to_owned(), (lo, hi));
        }
    }
    out
}

pub fn centre(b: ([f64; 3], [f64; 3])) -> [f64; 3] {
    scale(add(b.0, b.1), 0.5)
}

/// The fittings changed by hand: added, flipped, marked earlier, standing somewhere else now, or deleted (recorded,
/// but no longer in the design). Answers body names.
pub fn hand_changed(app: &mut crate::SolveApp) -> std::collections::BTreeSet<String> {
    let fittings = load(app);
    let now = boxes(app);
    fittings
        .iter()
        .filter(|(name, f)| match now.get(*name) {
            Some(b) => f.hand || f.flipped || f.moved(centre(*b)),
            None => true,
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// The fitting's bores where the fitting stands now: moved by the same amount it was moved.
pub fn bores_now(fitting: &Fitting, centre_now: [f64; 3]) -> Vec<Bore> {
    let delta = sub(centre_now, fitting.at);
    fitting
        .bores
        .iter()
        .zip(&fitting.frames)
        .map(|(bore, (fx, fy))| {
            let mut b = bore.clone();
            b.mouth = add(b.mouth, delta);
            // The machine list's board-frame position follows: along the board's x, and its y as seen from the red face.
            let ys = if b.face == Face::Back { -1.0 } else { 1.0 };
            b.local[0] += dot(delta, *fx);
            b.local[1] += ys * dot(delta, *fy);
            b
        })
        .collect()
}

/// A cam's housing on the other face of its board: through the board along its own axis, drilled back the other way.
pub fn flipped_cup(cup: &Bore, board: ([f64; 3], [f64; 3])) -> Bore {
    let axis = axis_of(cup.dir);
    let thick = board.1[axis] - board.0[axis];
    let mut b = cup.clone();
    b.mouth = add(cup.mouth, scale(cup.dir, thick));
    b.dir = scale(cup.dir, -1.0);
    b.face = match cup.face {
        Face::Front => Face::Back,
        Face::Back => Face::Front,
        Face::Edge => Face::Edge,
    };
    b.local[1] = b.panel[1] - b.local[1];
    b.local[2] = b.panel[2] - b.local[2];
    b
}

/// A cam's bolt or mate mirrored across its board when the housing flips to the other face: they ride a
/// fixed distance off the housing's face, so they follow it. `thick_axis` is the cup's board's thickness
/// axis; the bore's board-frame position moves the same way `bores_now` tracks a slide, and the bolt (which
/// shares the cup's board) mirrors its thickness coordinate.
fn flip_ridden_bore(
    mut bore: Bore,
    frame: ([f64; 3], [f64; 3]),
    pivot: [f64; 3],
    thick_axis: usize,
    cup_board: &str,
) -> Bore {
    let old = bore.mouth;
    bore.mouth[thick_axis] = 2.0 * pivot[thick_axis] - bore.mouth[thick_axis];
    let delta = sub(bore.mouth, old);
    let ys = if bore.face == Face::Back { -1.0 } else { 1.0 };
    bore.local[0] += dot(delta, frame.0);
    bore.local[1] += ys * dot(delta, frame.1);
    if bore.board == cup_board {
        bore.local[2] = bore.panel[2] - bore.local[2];
    }
    bore
}

/// What is wrong with one fitting's bores against the boards as they stand, in words; empty when nothing is.
pub fn check(
    name: &str,
    bores: &[Bore],
    boards: &std::collections::BTreeMap<String, ([f64; 3], [f64; 3])>,
    others: &[(String, Bore)],
) -> Vec<String> {
    let mut out = Vec::new();
    for bore in bores {
        let Some(board) = boards.get(&bore.board).copied() else {
            out.push(format!("{name}: its board {} is gone", bore.board));
            continue;
        };
        let thinnest = (0..3).map(|k| board.1[k] - board.0[k]).fold(f64::INFINITY, f64::min);
        if thinnest < ledger_holes::rules::MIN_HARDWARE_THICKNESS {
            out.push(format!("{name}: {} is 1/2\" board - no hardware", bore.board));
            continue;
        }
        let a = axis_of(bore.dir);
        let on_face = (bore.mouth[a] - board.0[a]).abs() < 0.5 || (bore.mouth[a] - board.1[a]).abs() < 0.5;
        // Into the board from the face the mouth is on: a bore pointing out of it is not on its joint either.
        let bottom = bore.mouth[a] + bore.dir[a] * bore.depth;
        let into = bottom > board.0[a] + FLOOR_MM - 1e-9 && bottom < board.1[a] - FLOOR_MM + 1e-9;
        if !on_face || !into {
            out.push(format!("{name}: off its joint - the {} bore no longer meets {}", short(bore.kind), bore.board));
            continue;
        }
        let r = bore.dia / 2.0;
        let rim = (0..3)
            .filter(|k| *k != a)
            .map(|k| (bore.mouth[k] - r - board.0[k]).min(board.1[k] - bore.mouth[k] - r))
            .fold(f64::INFINITY, f64::min);
        if rim < RIM_MM - 1e-9 {
            out.push(format!("{name}: the {} bore is {:.1} mm from the edge of {} (needs {RIM_MM:.0})", short(bore.kind), rim.max(0.0), bore.board));
        }
        for (other, theirs) in others {
            if other == name || theirs.board != bore.board {
                continue;
            }
            if overlaps(bore, theirs) {
                out.push(format!("{name}: hits {other} in {}", bore.board));
            }
        }
    }
    out.dedup();
    out
}

fn short(kind: HoleKind) -> &'static str {
    match kind {
        HoleKind::EdgeBolt => "bolt",
        HoleKind::CamCup => "housing",
        HoleKind::MatePreHole => "pilot",
        HoleKind::DowelEdge | HoleKind::DowelMateHole => "dowel",
        HoleKind::ShelfPin => "pin",
        HoleKind::Screw => "screw",
    }
}

/// Whether two bores' cylinders meet, by their boxes - bores here are square to the board, so the boxes are close.
fn overlaps(a: &Bore, b: &Bore) -> bool {
    let shape = |bore: &Bore| {
        let end = add(bore.mouth, scale(bore.dir, bore.depth));
        let ax = axis_of(bore.dir);
        let mut lo = [0.0; 3];
        let mut hi = [0.0; 3];
        for k in 0..3 {
            if k == ax {
                lo[k] = bore.mouth[k].min(end[k]);
                hi[k] = bore.mouth[k].max(end[k]);
            } else {
                lo[k] = bore.mouth[k] - bore.dia / 2.0;
                hi[k] = bore.mouth[k] + bore.dia / 2.0;
            }
        }
        (lo, hi)
    };
    let (p, q) = (shape(a), shape(b));
    (0..3).all(|k| p.1[k].min(q.1[k]) - p.0[k].max(q.0[k]) > 0.2)
}

/// Run `work` as **one** undo step labelled `label`: Calc Holes draws dozens of markers, and one Ctrl+Z must take the
/// whole press back, not one marker of it.
pub fn as_one_step<T>(app: &mut crate::SolveApp, label: &str, work: impl FnOnce(&mut crate::SolveApp) -> T) -> T {
    let start_doc = app.session.doc.clone();
    let start = app.session.undo.len();
    let out = work(app);
    let undo = &mut app.session.undo;
    if let Some(first) =
        (0..undo.len().min(start + 1)).rev().find(|&i| undo.get(i).is_some_and(|s| std::sync::Arc::ptr_eq(&s.doc, &start_doc)))
    {
        undo.truncate(first + 1);
        if let Some(step) = undo.get_mut(first) {
            step.label = label.to_owned();
        }
    }
    out
}

/// Whether a housing opens on a face the finished piece shows: the board face nearer the outside of the whole piece.
pub fn on_visible_face(cup: &Bore, board: ([f64; 3], [f64; 3]), piece: ([f64; 3], [f64; 3])) -> bool {
    let a = axis_of(cup.dir);
    let outer_is_lo = (board.0[a] - piece.0[a]) <= (piece.1[a] - board.1[a]);
    let outer = if outer_is_lo { board.0[a] } else { board.1[a] };
    (cup.mouth[a] - outer).abs() < 0.5
}

/// What [`flip`] did.
#[derive(Debug, Default)]
pub struct Flipped {
    pub done: Vec<String>,
    pub refused: Vec<String>,
    /// Flipped cams whose housing now opens on a face the finished piece shows.
    pub visible: Vec<String>,
}

/// Turn each chosen cam so its housing opens on the other face of its board.
pub fn flip(app: &mut crate::SolveApp, chosen: &[String]) -> Flipped {
    let mut fittings = load(app);
    let now = boxes(app);
    // The whole piece's extent, boards only: what "the outside" is measured against.
    let mut piece = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for (name, b) in &now {
        if crate::ledger_cams::is_marker_name(name) || crate::ledger_cams::stays_solid(name) {
            continue;
        }
        for k in 0..3 {
            piece.0[k] = piece.0[k].min(b.0[k]);
            piece.1[k] = piece.1[k].max(b.1[k]);
        }
    }
    let mut out = Flipped::default();
    let (done, refused) = (&mut out.done, &mut out.refused);
    for name in chosen {
        let (Some(fitting), Some(body)) = (fittings.get(name).cloned(), now.get(name).copied()) else {
            refused.push(name.clone());
            continue;
        };
        let bores = bores_now(&fitting, centre(body));
        let (Some(cup), Some(bolt)) =
            (bores.iter().position(|b| b.kind == HoleKind::CamCup), bores.iter().find(|b| b.kind == HoleKind::EdgeBolt).cloned())
        else {
            refused.push(name.clone());
            continue;
        };
        let Some(board) = now.get(&bores[cup].board).copied() else {
            refused.push(name.clone());
            continue;
        };
        // The model turns half a turn about the bolt's own line: the housing goes to the other face and still opens
        // toward its bolt, which stays where it is. The turn is about the board's mid-thickness, not the bolt's mouth
        // (which can sit a hair off the mid-plane), or the housing lands short of the opposite face.
        let thick_axis = axis_of(bores[cup].dir);
        let mut pivot = bolt.mouth;
        pivot[thick_axis] = (board.0[thick_axis] + board.1[thick_axis]) / 2.0;
        let moved = app.session.execute("solid.move", &json!({ "bodies": [name], "translate": scale(pivot, -1.0) })).is_ok()
            && app.session.execute("solid.move", &json!({ "bodies": [name], "axis": bolt.dir, "angle": "180 deg" })).is_ok()
            && app.session.execute("solid.move", &json!({ "bodies": [name], "translate": pivot })).is_ok();
        if !moved {
            refused.push(name.clone());
            continue;
        }
        let mut bores = bores;
        bores[cup] = flipped_cup(&bores[cup], board);
        // The bolt and its mate ride 9 mm off the housing's face, so the half-turn carries them across the
        // board with it too: mirror each about the board's mid-thickness.
        let cup_board = bores[cup].board.clone();
        for i in 0..bores.len() {
            if i == cup {
                continue;
            }
            let frame = fitting.frames.get(i).copied().unwrap_or(([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
            bores[i] = flip_ridden_bore(bores[i].clone(), frame, pivot, thick_axis, &cup_board);
        }
        if on_visible_face(&bores[cup], board, piece) {
            out.visible.push(name.clone());
        }
        let after = boxes(app);
        let at = after.get(name).copied().map_or(fitting.at, centre);
        let entry = fittings.get_mut(name).expect("loaded above");
        entry.bores = bores;
        entry.at = at;
        entry.flipped = !entry.flipped;
        entry.hand = true;
        done.push(name.clone());
    }
    save(app, &fittings);
    out
}

/// With snap on, a fitting moved off its joint is put back onto the joint's line: it keeps how far it slid along the
/// seam and loses the rest. Answers the fittings it put back.
pub fn snap_moved(app: &mut crate::SolveApp, moved: &[String]) -> Vec<String> {
    let fittings = load(app);
    if fittings.is_empty() {
        return Vec::new();
    }
    let now = boxes(app);
    let mut back = Vec::new();
    for name in moved {
        let (Some(fitting), Some(body)) = (fittings.get(name), now.get(name)) else { continue };
        if len(fitting.seam) < 0.5 {
            continue;
        }
        let delta = sub(centre(*body), fitting.at);
        let off = sub(delta, scale(fitting.seam, dot(delta, fitting.seam)));
        if len(off) > MOVED_MM
            && app.session.execute("solid.move", &json!({ "bodies": [name], "translate": scale(off, -1.0) })).is_ok()
        {
            back.push(name.clone());
        }
    }
    back
}

/// The line a fitting's joint runs along: the source board's axis that is neither the edge bore's drilling nor the
/// board's thickness.
pub fn seam_of(bores: &[&Bore], boards: &std::collections::BTreeMap<String, ([f64; 3], [f64; 3])>) -> [f64; 3] {
    let Some(edge) = bores.iter().find(|b| b.face == Face::Edge) else { return [0.0; 3] };
    let Some(board) = boards.get(&edge.board) else { return [0.0; 3] };
    let drill = axis_of(edge.dir);
    let size = |k: usize| board.1[k] - board.0[k];
    let thin = (0..3).filter(|k| *k != drill).min_by(|a, b| size(*a).total_cmp(&size(*b))).unwrap_or(2);
    let along = (0..3).find(|k| *k != drill && *k != thin).unwrap_or(0);
    let mut out = [0.0; 3];
    out[along] = 1.0;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bore(board: &str, kind: HoleKind, face: Face, mouth: [f64; 3], dir: [f64; 3], dia: f64, depth: f64) -> Bore {
        Bore { board: board.into(), kind, station: 1, face, mouth, dir, dia, depth, local: [100.0, 50.0, 0.0], panel: [600.0, 700.0, 19.05] }
    }

    fn boards() -> std::collections::BTreeMap<String, ([f64; 3], [f64; 3])> {
        [
            ("side".to_owned(), ([0.0, -600.0, 0.0], [19.05, 0.0, 736.6])),
            ("top".to_owned(), ([0.0, -600.0, 736.6], [1200.0, 0.0, 762.0])),
            ("thin".to_owned(), ([0.0, -600.0, 0.0], [12.7, 0.0, 736.6])),
        ]
        .into_iter()
        .collect()
    }

    /// A side-to-top cam: bolt up the side's top edge, housing in the side's inner face, pilot in the top.
    fn cam() -> Vec<Bore> {
        vec![
            bore("side", HoleKind::EdgeBolt, Face::Edge, [9.525, -300.0, 736.6], [0.0, 0.0, -1.0], 8.0, 34.0),
            bore("side", HoleKind::CamCup, Face::Front, [19.05, -300.0, 702.6], [-1.0, 0.0, 0.0], 15.0, 12.5),
            bore("top", HoleKind::MatePreHole, Face::Front, [9.525, -300.0, 736.6], [0.0, 0.0, 1.0], 5.0, 12.0),
        ]
    }

    #[test]
    fn the_rules_cam_passes_every_check() {
        assert!(check("cam 1", &cam(), &boards(), &[]).is_empty(), "{:?}", check("cam 1", &cam(), &boards(), &[]));
    }

    #[test]
    fn a_moved_cam_takes_its_bores_with_it() {
        let fitting = Fitting {
            key: "side|top|cam|0".into(),
            bores: cam(),
            at: [10.0, -300.0, 710.0],
            seam: [0.0, 1.0, 0.0],
            frames: vec![([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]); 3],
            flipped: false,
            hand: false,
        };
        let now = bores_now(&fitting, [10.0, -250.0, 710.0]);
        assert!(now.iter().all(|b| (b.mouth[1] + 250.0).abs() < 1e-9));
        assert!((now[0].local[0] - 150.0).abs() < 1e-9, "the machine list's x follows: {:?}", now[0].local);
        assert!(check("cam 1", &now, &boards(), &[]).is_empty());
        // Off the joint: lifted 20 mm, the bolt no longer meets the side's top edge.
        let off = bores_now(&fitting, [10.0, -300.0, 690.0]);
        assert!(check("cam 1", &off, &boards(), &[]).iter().any(|w| w.contains("off its joint")));
        // To the board's end: the bore's wall breaks the rim.
        let end = bores_now(&fitting, [10.0, -2.0, 710.0]);
        assert!(check("cam 1", &end, &boards(), &[]).iter().any(|w| w.contains("from the edge")));
    }

    #[test]
    fn a_flipped_housing_opens_on_the_other_face_of_the_same_board() {
        let cup = &cam()[1];
        let flipped = flipped_cup(cup, boards()["side"]);
        assert!((flipped.mouth[0] - 0.0).abs() < 1e-9 && flipped.dir == [1.0, 0.0, 0.0], "{flipped:?}");
        assert_eq!(flipped.face, Face::Back);
        assert_eq!(flipped.board, "side");
        let piece = ([0.0, -600.0, 0.0], [1200.0, 0.0, 762.0]);
        assert!(on_visible_face(&flipped, boards()["side"], piece), "the left side's outer face shows");
        assert!(!on_visible_face(cup, boards()["side"], piece), "the rule's housing is inside");
        let mut bores = cam();
        bores[1] = flipped;
        assert!(check("cam 1", &bores, &boards(), &[]).is_empty());
    }

    #[test]
    fn the_bolt_and_mate_follow_the_housing_across_the_board() {
        // A cam 1-style bolt: 9 mm off its board's front face (local[2] = 10.05), in the cup's board.
        let mut bolt = bore("panel - front", HoleKind::EdgeBolt, Face::Edge, [20.6375, -580.5, 57.8104], [1.0, 0.0, 0.0], 8.0, 34.0);
        bolt.local = [0.0, 678.7896, 10.05];
        bolt.panel = [1158.478, 729.5896, 19.05];
        let pivot = [20.6375, -581.025, 57.8104];
        let flipped = flip_ridden_bore(bolt, ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0]), pivot, 1, "panel - front");
        assert!((flipped.mouth[1] + 581.55).abs() < 1e-6, "{flipped:?}");
        assert!((flipped.local[2] - 9.0).abs() < 1e-6, "the bolt's thickness position mirrors: {flipped:?}");

        // The mate lives in the other board, so its thickness coordinate is left alone and its depth follows.
        let mut mate = bore("side (2)", HoleKind::MatePreHole, Face::Front, [20.6375, -580.5, 57.8104], [-1.0, 0.0, 0.0], 5.0, 12.0);
        mate.local = [678.7896, 19.575, 19.05];
        mate.panel = [729.5896, 600.075, 19.05];
        let flipped_mate = flip_ridden_bore(mate, ([0.0, 0.0, -1.0], [-0.0, 1.0, 0.0]), pivot, 1, "panel - front");
        assert!((flipped_mate.mouth[1] + 581.55).abs() < 1e-6, "{flipped_mate:?}");
        assert!((flipped_mate.local[1] - 18.525).abs() < 1e-6, "{flipped_mate:?}");
        assert!((flipped_mate.local[2] - 19.05).abs() < 1e-6, "the mate's thickness is left alone: {flipped_mate:?}");
    }

    #[test]
    fn half_inch_board_and_collisions_are_named() {
        let mut thin = cam();
        for b in thin.iter_mut().take(2) {
            b.board = "thin".into();
        }
        assert!(check("cam 1", &thin, &boards(), &[]).iter().any(|w| w.contains("1/2\"")));
        let other = cam()[1].clone();
        let hits = check("cam 1", &cam(), &boards(), &[("cam 2".into(), other)]);
        assert!(hits.iter().any(|w| w.contains("hits cam 2")), "{hits:?}");
    }

    #[test]
    fn a_seam_runs_along_the_edge_not_through_the_board() {
        let owned = cam();
        let refs: Vec<&Bore> = owned.iter().collect();
        assert_eq!(seam_of(&refs, &boards()), [0.0, 1.0, 0.0]);
    }
}
