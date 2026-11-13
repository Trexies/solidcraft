//! Bar handles: the shop's pull, put on a board by hand and moved where it goes; Calc Holes drills its bolt holes.
//!
//! The handle is the one on the SOL pieces (measured from `SOL-BF-2436-36-2D-1DW - 3D.stp`, 2026-11-13): a ⌀3/8" bar
//! 269.08 mm long on two ⌀5/16" posts 4 3/4" (120.65 mm) apart, the bar 1" off the board. Nothing places it by rule -
//! **Add ▾ → Handle** puts one in the middle of the picked board's outside face (along the board's long side: upright
//! on a door, across a drawer front) and the operator moves it. The body is the record: wherever it stands when Calc
//! Holes runs, each post gets a ⌀5 (M4) hole right through the board under it, drilled from the back so the bolt comes
//! through from inside (owner, 2026-11-13).

use serde_json::json;

use crate::ledger_cams::Box3;
use crate::ledger_fittings::{Bore, Step};

/// The bar: ⌀3/8" and 269.08 mm long.
pub const BAR_DIA: f64 = 9.525;
pub const BAR_LONG: f64 = 269.08;
/// The posts: ⌀5/16", centres 4 3/4" apart, centred on the bar's length.
pub const POST_DIA: f64 = 7.9375;
pub const POST_SPACING: f64 = 120.65;
/// From the board's face to the back of the bar: 1".
pub const STANDOFF: f64 = 25.4;
/// The bolt hole: ⌀5 for the M4 bolt, right through the board.
pub const BOLT_DIA: f64 = 5.0;

/// What the drawn handle's box is across its bar, out from the board, and along the bar.
const HANDLE_OUT: f64 = STANDOFF + BAR_DIA;

/// How far the handle's back may sit off the board's face (or into it - an imported drawing sinks its posts a few
/// millimetres into the door) and still be on that board, mm.
const ON_FACE_MM: f64 = 10.0;

/// A handle's bodies are named `handle N`.
pub fn is_handle_name(name: &str) -> bool {
    name.starts_with("handle ")
}

fn size(b: &Box3) -> [f64; 3] {
    [b.hi[0] - b.lo[0], b.hi[1] - b.lo[1], b.hi[2] - b.lo[2]]
}

fn centre(b: &Box3) -> [f64; 3] {
    [(b.lo[0] + b.hi[0]) / 2.0, (b.lo[1] + b.hi[1]) / 2.0, (b.lo[2] + b.hi[2]) / 2.0]
}

fn axes_by_size(b: &Box3) -> [usize; 3] {
    let s = size(b);
    let mut order = [0, 1, 2];
    order.sort_by(|x, y| s[*x].total_cmp(&s[*y]));
    order
}

/// Where a new handle goes on `board`: its middle on the board's outside face, along the board's long side.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// The point on the board's face under the middle of the bar.
    pub on_face: [f64; 3],
    /// Unit vector out of the face, toward the handle.
    pub out: [f64; 3],
    /// The world axis the bar runs along.
    pub along: usize,
}

/// Where a handle goes on `board` in a piece whose boards fill `piece` (lo, hi): the outside face is the one nearer
/// the piece's own outside on the board's thickness. Refused on a 1/2" board, by the owner's rule.
pub(crate) fn place(board: &Box3, piece: ([f64; 3], [f64; 3])) -> Result<Placed, String> {
    let [thin, _, long] = axes_by_size(board);
    if size(board)[thin] < ledger_holes::rules::MIN_HARDWARE_THICKNESS {
        return Err(format!("{} is 1/2\" board - no hardware goes into it", board.name));
    }
    if size(board)[long] < BAR_LONG.max(POST_SPACING + 2.0 * crate::ledger_hand::RIM_MM) {
        return Err(format!("{} is shorter than the handle ({BAR_LONG:.0} mm)", board.name));
    }
    let low_gap = board.lo[thin] - piece.0[thin];
    let high_gap = piece.1[thin] - board.hi[thin];
    let high = high_gap < low_gap;
    let mut on_face = centre(board);
    on_face[thin] = if high { board.hi[thin] } else { board.lo[thin] };
    let mut out = [0.0; 3];
    out[thin] = if high { 1.0 } else { -1.0 };
    // Upright on a door (taller than wide), across a drawer front: the board's long side in its face either way.
    Ok(Placed { on_face, out, along: long })
}

/// The commands that draw a handle at `placed`: the bar, then each post from the face into the bar's middle (never
/// through it, so no end shows on the bar's front).
pub fn steps(placed: &Placed) -> Vec<Step> {
    let out = placed.out;
    let at = |p: [f64; 3], k: f64| [p[0] + out[0] * k, p[1] + out[1] * k, p[2] + out[2] * k];
    let bar_mid = at(placed.on_face, STANDOFF + BAR_DIA / 2.0);
    let mut axis = [0.0; 3];
    axis[placed.along] = 1.0;
    let mut bar_base = bar_mid;
    bar_base[placed.along] -= BAR_LONG / 2.0;
    let mut steps = vec![Step {
        command: "solid.cylinder",
        params: json!({ "base": bar_base, "axis": axis, "diameter": BAR_DIA, "height": BAR_LONG }),
    }];
    for side in [-1.0, 1.0] {
        let mut foot = placed.on_face;
        foot[placed.along] += side * POST_SPACING / 2.0;
        steps.push(Step {
            command: "solid.cylinder",
            params: json!({ "base": foot, "axis": out, "diameter": POST_DIA, "height": STANDOFF + BAR_DIA / 2.0,
                "operation": "join" }),
        });
    }
    steps
}

/// Where a handle stands: the board it is on and its two bolt holes in that board.
#[derive(Debug, Clone, PartialEq)]
pub struct Mount {
    pub board: String,
    /// The bolt holes' mouths, on the board's back face under each post.
    pub mouths: [[f64; 3]; 2],
    /// Unit vector the drill travels: from the back face toward the handle.
    pub dir: [f64; 3],
    /// Right through: the board's thickness.
    pub depth: f64,
}

/// The board `handle` stands on and where its bolt holes go. The handle's box says how it lies: its longest side is
/// the bar, its middle side (bar plus standoff) points out of the board; the posts are [`POST_SPACING`] apart about
/// the bar's middle.
pub(crate) fn mount(handle: &Box3, boards: &[Box3]) -> Result<Mount, String> {
    let [_, out_axis, along] = axes_by_size(handle);
    let mid = centre(handle);
    let mut posts = [mid, mid];
    posts[0][along] -= POST_SPACING / 2.0;
    posts[1][along] += POST_SPACING / 2.0;
    for board in boards {
        let thin = axes_by_size(board)[0];
        if thin != out_axis {
            continue;
        }
        // The handle's back against the board's front or its back.
        let side = if (handle.lo[out_axis] - board.hi[out_axis]).abs() <= ON_FACE_MM
            && handle.lo[out_axis] > board.hi[out_axis] - ON_FACE_MM
        {
            1.0
        } else if (board.lo[out_axis] - handle.hi[out_axis]).abs() <= ON_FACE_MM
            && handle.hi[out_axis] < board.lo[out_axis] + ON_FACE_MM
        {
            -1.0
        } else {
            continue;
        };
        let inside = |p: &[f64; 3]| (0..3).filter(|k| *k != out_axis).all(|k| board.lo[k] <= p[k] && p[k] <= board.hi[k]);
        if !posts.iter().all(inside) {
            continue;
        }
        let back = if side > 0.0 { board.lo[out_axis] } else { board.hi[out_axis] };
        let mut mouths = posts;
        for mouth in &mut mouths {
            mouth[out_axis] = back;
        }
        let mut dir = [0.0; 3];
        dir[out_axis] = side;
        return Ok(Mount { board: board.name.clone(), mouths, dir, depth: board.hi[out_axis] - board.lo[out_axis] });
    }
    Err(format!("{} is not on a board - both posts must sit on one board's face", handle.name))
}

/// Every handle's bolt holes, as bores on the engine's boards (`panels`), and what is wrong with any handle that has
/// none: off every board, or on a 1/2" one.
pub(crate) fn bolts(bodies: &[Box3], panels: &[ledger_holes::Panel]) -> (Vec<Bore>, Vec<String>) {
    let boards: Vec<Box3> = bodies
        .iter()
        .filter(|b| !crate::ledger_cams::stays_solid(&b.name))
        .map(|b| Box3 { name: b.name.clone(), lo: b.lo, hi: b.hi })
        .collect();
    let mut bores = Vec::new();
    let mut problems = Vec::new();
    for handle in bodies.iter().filter(|b| is_handle_name(&b.name)) {
        let mount = match mount(handle, &boards) {
            Ok(mount) => mount,
            Err(why) => {
                problems.push(why);
                continue;
            }
        };
        if mount.depth < ledger_holes::rules::MIN_HARDWARE_THICKNESS {
            problems.push(format!("{}: {} is 1/2\" board - no hardware", handle.name, mount.board));
            continue;
        }
        let Some(panel) = panels.iter().find(|p| p.name == mount.board) else {
            problems.push(format!("{}: {} is not a board the hole rules can read", handle.name, mount.board));
            continue;
        };
        for mouth in mount.mouths {
            bores.push(crate::ledger_cams::bore_at(
                panel,
                ledger_holes::HoleKind::HandleBolt,
                mouth,
                mount.dir,
                BOLT_DIA,
                mount.depth,
                0,
            ));
        }
    }
    (bores, problems)
}

/// The size a handle drawn by [`steps`] comes out: across the bar, out from the board, along the bar.
pub fn drawn_size() -> [f64; 3] {
    [BAR_DIA, HANDLE_OUT, BAR_LONG]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, lo: [f64; 3], hi: [f64; 3]) -> Box3 {
        Box3 { name: name.into(), lo, hi }
    }

    /// The BF-2436's left door and drawer front, as drawn: the front of the piece is low y.
    fn door() -> Box3 {
        b("panel - front", [1.5875, -593.725, 110.1979], [450.05625, -574.675, 735.0125])
    }

    fn drawer() -> Box3 {
        b("drawer front", [1.6, -593.725, 736.6], [900.1, -574.675, 889.0])
    }

    fn piece() -> ([f64; 3], [f64; 3]) {
        ([0.0, -593.725, 0.0], [902.0, 0.0, 914.0])
    }

    #[test]
    fn a_new_handle_goes_upright_on_a_door_and_across_a_drawer_on_the_outside() {
        let on_door = place(&door(), piece()).expect("placed");
        assert_eq!(on_door.along, 2, "upright on a door");
        assert_eq!(on_door.out, [0.0, -1.0, 0.0], "out of the front");
        assert!((on_door.on_face[1] + 593.725).abs() < 1e-9);
        let on_drawer = place(&drawer(), piece()).expect("placed");
        assert_eq!(on_drawer.along, 0, "across a drawer front");
    }

    #[test]
    fn a_half_inch_board_takes_no_handle() {
        let thin = b("door", [0.0, -12.7, 0.0], [450.0, 0.0, 600.0]);
        assert!(place(&thin, ([0.0, -12.7, 0.0], [450.0, 300.0, 600.0])).unwrap_err().contains("1/2\""));
    }

    /// The drawn handle, read back by its box, mounts on the board it was drawn on, with one hole under each post.
    #[test]
    fn a_drawn_handle_bolts_through_its_board_from_the_back() {
        let placed = place(&door(), piece()).expect("placed");
        let [across, out, along] = drawn_size();
        let mid = placed.on_face;
        let handle = b(
            "handle 1",
            [mid[0] - across / 2.0, mid[1] - out, mid[2] - along / 2.0],
            [mid[0] + across / 2.0, mid[1], mid[2] + along / 2.0],
        );
        let mount = mount(&handle, &[door(), drawer()]).expect("on the door");
        assert_eq!(mount.board, "panel - front");
        assert_eq!(mount.dir, [0.0, -1.0, 0.0], "from the back toward the handle");
        assert!((mount.depth - 19.05).abs() < 1e-6, "right through");
        for mouth in mount.mouths {
            assert!((mouth[1] + 574.675).abs() < 1e-9, "on the back face");
        }
        assert!((mount.mouths[1][2] - mount.mouths[0][2] - POST_SPACING).abs() < 1e-9);
        assert!((mount.mouths[0][2] + mount.mouths[1][2] - 2.0 * mid[2]).abs() < 1e-9, "centred on the bar");
    }

    /// The handle as the drawing has it: posts sunk 7.5 mm into the door, still that door's handle.
    #[test]
    fn an_imported_handle_with_its_posts_in_the_door_is_on_the_door() {
        let handle = b("handle 1", [420.0, -628.65, 470.9], [429.5, -586.18, 739.98]);
        let mount = mount(&handle, &[door()]).expect("on the door");
        assert!((mount.mouths[0][2] - (605.44 - 60.325)).abs() < 0.01);
    }

    #[test]
    fn a_handle_off_every_board_is_named() {
        let handle = b("handle 2", [420.0, -700.0, 470.9], [429.5, -660.0, 739.98]);
        assert!(mount(&handle, &[door()]).unwrap_err().contains("handle 2 is not on a board"));
    }
}
