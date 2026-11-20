//! Hardware the shop designed in SolidCraft (Ledger's Hardware tab, owner 2026-11-19).
//!
//! An admin models a clamp, a bracket at its real size; each hole it drills is a **cutter**, a cylinder named "cut …".
//! Ledger keeps every changed save as a version (`hardware/HW-<id>-v<N>.solidcraft`) and publishes the newest with its
//! cutters, and every older version's cutters, in `hardware.json`.
//!
//! Here: `Add ▾` inserts the newest version as a linked part, set on the selected board; a placement is any component
//! whose linked file is one of those versions. Calc Holes drills each placement's cutters, in the version the placement
//! was given - a product keeps the hardware it was made with (owner: an edit affects only products from then on), so
//! nothing here ever refreshes a link. The cutters are moved by the placement's own transform, so a piece turned to any
//! angle drills where it now points. Hardware is never a board: its bodies stay out of the camlock and edgebanding rules.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::ledger_fittings::Bore;
use crate::ledger_cams::Box3;

/// Overlap below this (mm) is a touch, not a hole.
const TOUCH: f64 = 0.05;

/// One hole a piece of hardware drills: its centre line in the hardware's own frame, and its diameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Cutter {
    pub name: String,
    pub diameter: f64,
    pub from: [f64; 3],
    pub to: [f64; 3],
}

/// A piece of hardware as Ledger published it: its newest version, and the cutters of every version.
#[derive(Clone, Debug, PartialEq)]
pub struct Designed {
    pub id: i64,
    pub name: String,
    pub version: u32,
    /// The newest version's file, relative to Ledger's data folder.
    pub file: String,
    pub cutters: Vec<Cutter>,
    pub history: BTreeMap<u32, Vec<Cutter>>,
}

impl Designed {
    /// The cutters of one version of it.
    pub fn cutters_of(&self, version: u32) -> Option<&[Cutter]> {
        if version == self.version {
            return Some(&self.cutters);
        }
        self.history.get(&version).map(Vec::as_slice)
    }
}

fn point(value: &Value) -> Option<[f64; 3]> {
    let list = value.as_array()?;
    Some([list.first()?.as_f64()?, list.get(1)?.as_f64()?, list.get(2)?.as_f64()?])
}

fn cutters_from(value: &Value) -> Vec<Cutter> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|cut| {
                    Some(Cutter {
                        name: cut.get("name")?.as_str()?.to_owned(),
                        diameter: cut.get("diameter")?.as_f64()?,
                        from: point(cut.get("from")?)?,
                        to: point(cut.get("to")?)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The designed hardware in Ledger's `hardware.json` text: rows with a design only.
pub fn parse(text: &str) -> Vec<Designed> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(rows) = value.get("hardware").and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let design = row.get("design").filter(|design| design.is_object())?;
            Some(Designed {
                id: row.get("id")?.as_i64()?,
                name: row.get("name")?.as_str()?.to_owned(),
                version: u32::try_from(design.get("version")?.as_u64()?).ok()?,
                file: design.get("file")?.as_str()?.to_owned(),
                cutters: cutters_from(design.get("cutters").unwrap_or(&Value::Null)),
                history: design
                    .get("history")
                    .and_then(Value::as_object)
                    .map(|map| map.iter().filter_map(|(key, cuts)| Some((key.parse().ok()?, cutters_from(cuts)))).collect())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// The designed hardware Ledger published beside the boards.
pub fn list() -> Vec<Designed> {
    std::fs::read_to_string(crate::ledger_boards::file().with_file_name("hardware.json")).map(|text| parse(&text)).unwrap_or_default()
}

/// Which hardware and version a linked file is: `…/HW-3-v2.solidcraft` → (3, 2).
pub fn version_of(path: &str) -> Option<(i64, u32)> {
    let name = path.rsplit(['/', '\\']).next()?;
    let (id, version) = name.strip_suffix(".solidcraft")?.strip_prefix("HW-")?.split_once("-v")?;
    Some((id.parse().ok()?, version.parse().ok()?))
}

/// One piece of designed hardware placed in the design.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// The occurrence's name, as the tree shows it (`Test clamp v1:1`).
    pub occurrence: String,
    pub id: i64,
    pub version: u32,
    /// Its bodies' names in this design (renamed on a clash: `cut 1 (2)`).
    pub bodies: Vec<String>,
    /// Where it stands: rows are the images of its x, y and z axes, then its origin.
    pub transform: [[f64; 4]; 4],
}

fn matrix(value: &Value) -> Option<[[f64; 4]; 4]> {
    let rows = value.as_array()?;
    let mut out = [[0.0; 4]; 4];
    for (i, row) in rows.iter().take(4).enumerate() {
        let row = row.as_array()?;
        for (j, slot) in out[i].iter_mut().enumerate() {
            *slot = row.get(j)?.as_f64()?;
        }
    }
    Some(out)
}

/// The placements, from `component.links` and `component.list {compact, bodies}`.
pub fn placed_in(links: &Value, list: &Value) -> Vec<Placed> {
    let files: BTreeMap<u64, (i64, u32)> = links
        .get("links")
        .and_then(Value::as_array)
        .map(|links| {
            links
                .iter()
                .filter_map(|link| {
                    let component = link.get("component")?.as_u64()?;
                    let file = link.get("file").or_else(|| link.get("path"))?.as_str()?;
                    Some((component, version_of(file)?))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut out = Vec::new();
    for component in list.get("components").and_then(Value::as_array).into_iter().flatten() {
        let Some(&(id, version)) = component.get("id").and_then(Value::as_u64).and_then(|c| files.get(&c)) else {
            continue;
        };
        let bodies: Vec<String> = component
            .get("bodies")
            .and_then(Value::as_array)
            .map(|bodies| bodies.iter().filter_map(|b| b.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        for occurrence in component.get("occurrences").and_then(Value::as_array).into_iter().flatten() {
            let (Some(name), Some(transform)) = (occurrence.get("name").and_then(Value::as_str), occurrence.get("transform").and_then(matrix)) else {
                continue;
            };
            out.push(Placed { occurrence: name.to_owned(), id, version, bodies: bodies.clone(), transform });
        }
    }
    out
}

/// The designed hardware placed in this design.
pub fn placed(session: &mut solidcraft_engine::Session) -> Vec<Placed> {
    let Ok(links) = session.execute("component.links", &json!({})) else {
        return Vec::new();
    };
    let Ok(list) = session.execute("component.list", &json!({ "compact": true, "bodies": true })) else {
        return Vec::new();
    };
    placed_in(&links, &list)
}

/// The bodies of designed hardware in the design last looked at, and which design and revision that was.
static BODIES: Mutex<Option<(String, u64, BTreeSet<String>)>> = Mutex::new(None);

/// Read again which bodies are designed hardware, when the design changed since the last look.
pub fn refresh(app: &mut crate::SolveApp) {
    let document = app.session.document_id.clone();
    let revision = app.session.revision;
    {
        let seen = BODIES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if seen.as_ref().is_some_and(|(d, r, _)| *d == document && *r == revision) {
            return;
        }
    }
    let bodies: BTreeSet<String> = placed(&mut app.session).into_iter().flat_map(|placed| placed.bodies).collect();
    *BODIES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((document, app.session.revision, bodies));
}

/// Whether a body belongs to designed hardware: never a board.
pub fn is_hardware_body(name: &str) -> bool {
    BODIES.lock().ok().and_then(|seen| seen.as_ref().map(|(_, _, bodies)| bodies.contains(name))).unwrap_or(false)
}

/// A point of the hardware's own frame, where the placement puts it.
fn apply(transform: &[[f64; 4]; 4], p: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (j, slot) in out.iter_mut().enumerate() {
        *slot = p[0] * transform[0][j] + p[1] * transform[1][j] + p[2] * transform[2][j] + transform[3][j];
    }
    out
}

/// The share of the line a → b inside the box, as fractions of it.
fn clip(a: [f64; 3], b: [f64; 3], lo: [f64; 3], hi: [f64; 3]) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for i in 0..3 {
        let d = b[i] - a[i];
        if d.abs() < 1e-9 {
            if a[i] < lo[i] - 1e-6 || a[i] > hi[i] + 1e-6 {
                return None;
            }
            continue;
        }
        let (mut u, mut v) = ((lo[i] - a[i]) / d, (hi[i] - a[i]) / d);
        if u > v {
            std::mem::swap(&mut u, &mut v);
        }
        t0 = t0.max(u);
        t1 = t1.min(v);
        if t0 >= t1 {
            return None;
        }
    }
    Some((t0, t1))
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// One cutter's hole in one board: where it opens, which way it runs, how deep.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub board: String,
    pub mouth: [f64; 3],
    pub dir: [f64; 3],
    pub depth: f64,
}

/// Where a cutter running from `near` (the hardware's end) to `far` cuts each board. The mouth is where it enters the
/// board from the hardware's side - on the board's face even when the cutter starts inside it - and the depth runs to
/// where the cutter ends or the board does.
pub(crate) fn cuts(near: [f64; 3], far: [f64; 3], boards: &[Box3]) -> Vec<Cut> {
    let length = distance(near, far);
    if length < TOUCH {
        return Vec::new();
    }
    let dir = [(far[0] - near[0]) / length, (far[1] - near[1]) / length, (far[2] - near[2]) / length];
    let mut out = Vec::new();
    for board in boards {
        let Some((t0, t1)) = clip(near, far, board.lo, board.hi) else {
            continue;
        };
        let end = lerp(near, far, t1);
        let mut mouth = lerp(near, far, t0);
        if t0 <= 1e-9 {
            // The cutter starts inside the board: the hole still opens on the face it came through.
            let back = [near[0] - dir[0] * 1.0e4, near[1] - dir[1] * 1.0e4, near[2] - dir[2] * 1.0e4];
            if let Some((s0, _)) = clip(back, near, board.lo, board.hi) {
                mouth = lerp(back, near, s0);
            }
        }
        let depth = distance(mouth, end);
        if depth < TOUCH {
            continue;
        }
        out.push(Cut { board: board.name.clone(), mouth, dir, depth: (depth * 100.0).round() / 100.0 });
    }
    out
}

fn centre(b: &Box3) -> [f64; 3] {
    [(b.lo[0] + b.hi[0]) / 2.0, (b.lo[1] + b.hi[1]) / 2.0, (b.lo[2] + b.hi[2]) / 2.0]
}

/// The holes every placed piece of designed hardware drills on the engine's boards (`panels`), and what is wrong with
/// any piece: a version Ledger has no cuts for, a cutter touching no board, a 1/2" board.
pub(crate) fn holes(placed: &[Placed], designed: &[Designed], bodies: &[Box3], panels: &[ledger_holes::Panel]) -> (Vec<Bore>, Vec<String>) {
    let hardware: BTreeSet<&str> = placed.iter().flat_map(|p| p.bodies.iter().map(String::as_str)).collect();
    let boards: Vec<Box3> =
        bodies.iter().filter(|b| !hardware.contains(b.name.as_str()) && !crate::ledger_cams::stays_solid(&b.name)).map(|b| Box3 { name: b.name.clone(), lo: b.lo, hi: b.hi }).collect();
    let mut bores = Vec::new();
    let mut problems = Vec::new();
    for piece in placed {
        let Some(cutters) = designed.iter().find(|d| d.id == piece.id).and_then(|d| d.cutters_of(piece.version)) else {
            problems.push(format!("{}: Ledger has no cuts for version {} of this hardware", piece.occurrence, piece.version));
            continue;
        };
        // The hardware's own body, to tell which end of each cutter is the hardware's.
        let solid: Vec<[f64; 3]> =
            bodies.iter().filter(|b| piece.bodies.contains(&b.name) && !b.name.to_lowercase().starts_with("cut")).map(centre).collect();
        let anchor = if solid.is_empty() {
            [piece.transform[3][0], piece.transform[3][1], piece.transform[3][2]]
        } else {
            let n = solid.len() as f64;
            [0, 1, 2].map(|i| solid.iter().map(|c| c[i]).sum::<f64>() / n)
        };
        let mut drilled = 0;
        for cutter in cutters {
            let (a, b) = (apply(&piece.transform, cutter.from), apply(&piece.transform, cutter.to));
            let (near, far) = if distance(a, anchor) <= distance(b, anchor) { (a, b) } else { (b, a) };
            let found = cuts(near, far, &boards);
            if found.is_empty() {
                problems.push(format!("{} {}: touches no board", piece.occurrence, cutter.name));
                continue;
            }
            for cut in found {
                let board = boards.iter().find(|b| b.name == cut.board);
                let thin = board.map_or(0.0, |b| (0..3).map(|i| b.hi[i] - b.lo[i]).fold(f64::MAX, f64::min));
                if thin < ledger_holes::rules::MIN_HARDWARE_THICKNESS {
                    problems.push(format!("{}: {} is 1/2\" board - no hardware", piece.occurrence, cut.board));
                    continue;
                }
                let Some(panel) = panels.iter().find(|p| p.name == cut.board) else {
                    problems.push(format!("{}: {} is not a board the hole rules can read", piece.occurrence, cut.board));
                    continue;
                };
                bores.push(crate::ledger_cams::bore_at(panel, ledger_holes::HoleKind::Designed, cut.mouth, cut.dir, cutter.diameter, cut.depth, 0));
                drilled += 1;
            }
        }
        let _ = drilled;
    }
    (bores, problems)
}

/// Put the newest version of `piece` into the design as a linked part, on top of the selected board (at its centre) or
/// at the origin. Refused inside a hardware design itself.
pub fn insert(app: &mut crate::SolveApp, piece: &Designed) -> Result<String, String> {
    if crate::ledger_save::tab_name(&app.session).starts_with("HW-") {
        return Err("this tab is a hardware design - add hardware to a product".to_owned());
    }
    let folder = crate::ledger_boards::file().parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let path = folder.join(piece.file.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !path.is_file() {
        return Err(format!("{} v{}: its file is not on this PC yet ({})", piece.name, piece.version, path.display()));
    }
    let chosen = crate::ledger_resize::selected(app);
    let at = app
        .session
        .execute("inspect.measure", &json!({}))
        .ok()
        .map(|measured| crate::ledger_cams::boxes(&measured))
        .and_then(|bodies| bodies.into_iter().find(|b| chosen.contains(&b.name) && !b.is_marker() && !crate::ledger_cams::stays_solid(&b.name)))
        .map_or([0.0, 0.0, 0.0], |board| [(board.lo[0] + board.hi[0]) / 2.0, (board.lo[1] + board.hi[1]) / 2.0, board.hi[2]]);
    // A name of its own, so a second one in the tree and in Calc Holes' messages is told apart: "Clamp v1 (2)".
    let base = format!("{} v{}", piece.name, piece.version);
    let taken: Vec<String> = app
        .session
        .execute("component.list", &json!({ "compact": true }))
        .ok()
        .and_then(|list| list.get("components").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| c.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let name = (1..)
        .map(|n| if n == 1 { base.clone() } else { format!("{base} ({n})") })
        .find(|name| !taken.contains(name))
        .unwrap_or(base);
    app.run("component.insert_design", json!({ "path": path.to_string_lossy(), "name": name, "translate": at }))?;
    // The board it was set on stays unselected: a selection narrows Calc Holes to the boards picked.
    let _ = app.run("select.clear", json!({}));
    refresh(app);
    Ok(format!("added {name} - move and turn it into place, then press Calc Holes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(name: &str, lo: [f64; 3], hi: [f64; 3]) -> Box3 {
        Box3 { name: name.into(), lo, hi }
    }

    /// Stage 0's test product: a 3/4" top, a 3/4" front rail standing under its front edge, a 1/2" back.
    fn product() -> Vec<Box3> {
        vec![
            board("top", [0.0, 0.0, 0.0], [600.0, 400.0, 19.0]),
            board("front rail", [0.0, -19.0, -300.0], [600.0, 0.0, 0.0]),
            board("back", [0.0, 400.0, -300.0], [600.0, 412.7, 0.0]),
        ]
    }

    fn clamp(version: u32, diameter: f64) -> Designed {
        let cut = |name: &str, x: f64| Cutter { name: name.into(), diameter, from: [x, 10.0, -12.0], to: [x, 10.0, 0.0] };
        Designed {
            id: 8,
            name: "Test clamp".into(),
            version,
            file: format!("hardware/HW-8-v{version}.solidcraft"),
            cutters: vec![cut("cut 1", 10.0), cut("cut 2", 30.0)],
            history: BTreeMap::new(),
        }
    }

    fn at(translate: [f64; 3]) -> [[f64; 4]; 4] {
        [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [translate[0], translate[1], translate[2], 1.0]]
    }

    #[test]
    fn a_linked_file_names_its_hardware_and_version() {
        assert_eq!(version_of(r"D:\Ledger\data\hardware\HW-8-v2.solidcraft"), Some((8, 2)));
        assert_eq!(version_of("hardware/HW-12-v10.solidcraft"), Some((12, 10)));
        assert_eq!(version_of("clamp.solidcraft"), None);
    }

    #[test]
    fn hardware_json_gives_every_versions_cuts() {
        let text = r#"{"format":"ledger-hardware/1","hardware":[
            {"id":8,"name":"Test clamp","kind":"other","design":{"version":2,"file":"hardware/HW-8-v2.solidcraft",
             "cutters":[{"name":"cut 1","diameter":8.0,"from":[10,10,-12],"to":[10,10,0]}],
             "history":{"1":[{"name":"cut 1","diameter":5.0,"from":[10,10,-12],"to":[10,10,0]}]}}},
            {"id":2,"name":"Cam","kind":"cam","design":null}]}"#;
        let list = parse(text);
        assert_eq!(list.len(), 1, "a row without a design is not designed hardware");
        assert_eq!(list[0].cutters_of(2).unwrap()[0].diameter, 8.0);
        assert_eq!(list[0].cutters_of(1).unwrap()[0].diameter, 5.0, "a product holding v1 drills v1's holes");
        assert_eq!(list[0].cutters_of(3), None);
    }

    #[test]
    fn placements_are_the_components_linked_to_a_version() {
        let links = json!({"links": [
            {"component": 4, "file": "D:\\Ledger\\data\\hardware\\HW-8-v1.solidcraft", "name": "Test clamp v1"},
            {"component": 9, "file": "D:\\elsewhere\\chair.solidcraft", "name": "Chair"}]});
        let list = json!({"components": [
            {"id": 0, "name": "desk", "bodies": ["top"], "occurrences": []},
            {"id": 4, "name": "Test clamp v1", "bodies": ["Clamp", "cut 1", "cut 2"],
             "occurrences": [{"name": "Test clamp v1:1", "transform": at([100.0, 100.0, 19.0])}]},
            {"id": 9, "name": "Chair", "bodies": ["seat"], "occurrences": [{"name": "Chair:1", "transform": at([0.0; 3])}]}]});
        let placed = placed_in(&links, &list);
        assert_eq!(placed.len(), 1);
        assert_eq!((placed[0].id, placed[0].version), (8, 1));
        assert_eq!(placed[0].bodies, ["Clamp", "cut 1", "cut 2"]);
    }

    /// Placement A of stage 0: on the top, its cutters 12 mm down into the 19 mm top.
    #[test]
    fn a_cutter_drills_the_board_it_points_into() {
        let piece = clamp(1, 5.0);
        let t = at([100.0, 100.0, 19.0]);
        let near = apply(&t, piece.cutters[0].to);
        let far = apply(&t, piece.cutters[0].from);
        let found = cuts(near, far, &product());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].board, "top");
        assert_eq!(found[0].mouth, [110.0, 110.0, 19.0]);
        assert_eq!(found[0].dir, [0.0, 0.0, -1.0]);
        assert_eq!(found[0].depth, 12.0);
    }

    /// Turned a quarter about z (stage 0's rotation): the cutters follow the turn.
    #[test]
    fn a_turned_piece_drills_where_its_cutters_now_are() {
        let piece = clamp(1, 5.0);
        let t = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [120.0, 100.0, 19.0, 1.0]];
        let mouths: Vec<[f64; 3]> = piece.cutters.iter().map(|c| cuts(apply(&t, c.to), apply(&t, c.from), &product())[0].mouth).collect();
        assert_eq!(mouths, [[110.0, 110.0, 19.0], [110.0, 130.0, 19.0]]);
    }

    /// A cutter that starts inside the board (the hardware sunk a little) still opens on the board's face.
    #[test]
    fn a_hole_opens_on_the_face_even_when_the_cutter_starts_inside() {
        let found = cuts([50.0, 50.0, 17.0], [50.0, 50.0, 5.0], &product());
        assert_eq!(found[0].mouth, [50.0, 50.0, 19.0]);
        assert_eq!(found[0].depth, 14.0);
    }

    #[test]
    fn a_piece_off_the_boards_or_on_a_half_inch_one_drills_nothing_and_says_why() {
        let designed = [clamp(1, 5.0)];
        let place = |translate: [f64; 3], name: &str| Placed {
            occurrence: name.into(),
            id: 8,
            version: 1,
            bodies: vec!["Clamp".into(), "cut 1".into(), "cut 2".into()],
            transform: at(translate),
        };
        // Off in the air (placement D), and on the 1/2" back's top edge (placement C).
        let (bores, problems) =
            holes(&[place([700.0, 0.0, 100.0], "Clamp D:1"), place([200.0, 396.35, 0.0], "Clamp C:1")], &designed, &product(), &[]);
        assert!(bores.is_empty());
        assert!(problems.iter().any(|p| p.starts_with("Clamp D:1 cut 1: touches no board")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("back is 1/2\" board")), "{problems:?}");
        // A version Ledger has no cuts for is said, not guessed.
        let (_, problems) = holes(&[Placed { version: 4, ..place([100.0, 100.0, 19.0], "Clamp A:1") }], &designed, &product(), &[]);
        assert_eq!(problems, ["Clamp A:1: Ledger has no cuts for version 4 of this hardware"]);
    }
}
