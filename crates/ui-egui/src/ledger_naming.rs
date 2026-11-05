//! Naming the bodies of an imported drawing, from the shape and the place of each one.
//!
//! A shop DWG carries no names of its own - every solid in the CAD team's file sits on layer `0` - so a body arrives as
//! `Part (12)`. This names what the geometry can name, and leaves the rest alone: a body that fits two descriptions, or none,
//! keeps its number, because a wrong "leg" is taken for fact while a number is plainly unknown.
//!
//! The rules are the ones Ledger's import pass uses (Ledger, `src/sc/naming.rs`). They live here as well so the ribbon can
//! re-run them on a design that is already open, without a trip through Ledger.

use serde_json::{json, Value};

/// A body's box, in millimetres, in the assembly's own coordinates.
struct Box3 {
    name: String,
    lo: [f64; 3],
    hi: [f64; 3],
}

impl Box3 {
    fn size(&self) -> [f64; 3] {
        [self.hi[0] - self.lo[0], self.hi[1] - self.lo[1], self.hi[2] - self.lo[2]]
    }
    fn centre(&self) -> [f64; 3] {
        [(self.lo[0] + self.hi[0]) / 2.0, (self.lo[1] + self.hi[1]) / 2.0, (self.lo[2] + self.hi[2]) / 2.0]
    }
}

/// The thinnest a panel can be and still be a panel: below this the body is a pin, a sliver of edgebanding, or a marking.
const THINNEST_BOARD_MM: f64 = 5.0;
/// The thickest a shop board gets (1-1/8"), with room for a drawing that measures a hair over.
const THICKEST_BOARD_MM: f64 = 40.0;
/// A board is thin against its own middle dimension; a solid block is not.
const BOARD_RATIO: f64 = 0.30;

/// What a body is, when the shape says one thing and no other.
fn describe(body: &Box3, whole: [f64; 3], lo: [f64; 3], hi: [f64; 3]) -> Option<String> {
    let size = body.size();
    let centre = body.centre();
    let (ax, ay, az) = (whole[0], whole[1], whole[2]);
    if ax <= 0.0 || ay <= 0.0 || az <= 0.0 {
        return None;
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|a, b| size[*a].total_cmp(&size[*b]));
    let thin_axis = order[0];
    let thin = size[thin_axis];
    let middle = size[order[1]];
    // The board gate: nothing is a shelf until it measures like a board.
    if !(THINNEST_BOARD_MM..=THICKEST_BOARD_MM).contains(&thin) || thin > middle * BOARD_RATIO {
        return None;
    }
    let mid = [whole[0] / 2.0, whole[1] / 2.0];
    let at_low = |axis: usize, share: f64| centre[axis] <= lo[axis] + whole[axis] * share;
    let at_high = |axis: usize, share: f64| centre[axis] >= hi[axis] - whole[axis] * share;
    let spans = |axis: usize, share: f64| size[axis] >= whole[axis] * share;
    match thin_axis {
        2 => {
            if spans(0, 0.75) && spans(1, 0.75) && at_high(2, 0.20) {
                Some("top".to_owned())
            } else if spans(0, 0.40) || spans(1, 0.40) {
                Some("shelf".to_owned())
            } else {
                None
            }
        }
        0 => {
            if size[2] >= az * 0.50 && spans(1, 0.60) {
                Some("side".to_owned())
            } else if size[2] >= az * 0.50 && size[1] <= ay * 0.25 {
                Some(format!(
                    "leg - {} {}",
                    if centre[1] < mid[1] { "front" } else { "rear" },
                    if centre[0] < mid[0] { "left" } else { "right" }
                ))
            } else if size[2] <= az * 0.25 && spans(1, 0.40) {
                Some("rail - side".to_owned())
            } else {
                None
            }
        }
        _ => {
            if size[2] >= az * 0.50 && at_high(1, 0.08) {
                Some("panel - rear".to_owned())
            } else if size[2] >= az * 0.50 && at_low(1, 0.08) {
                Some("panel - front".to_owned())
            } else if size[2] >= az * 0.50 && spans(0, 0.60) {
                Some("panel - rear".to_owned())
            } else if size[2] >= az * 0.50 {
                Some("divider".to_owned())
            } else if size[2] <= az * 0.25 && spans(0, 0.40) {
                Some(format!(
                    "rail - {}",
                    if centre[1] < mid[1] { "front" } else { "rear" }
                ))
            } else {
                None
            }
        }
    }
}

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

/// Name every body the shape can name. Answers (named, left as numbers).
pub fn name_parts(app: &mut crate::SolveApp) -> (usize, usize) {
    let Ok(measured) = app.session.execute("inspect.measure", &json!({})) else {
        return (0, 0);
    };
    let bodies = boxes(&measured);
    if bodies.is_empty() {
        return (0, 0);
    }
    let mut lo = bodies[0].lo;
    let mut hi = bodies[0].hi;
    for body in &bodies {
        for axis in 0..3 {
            lo[axis] = lo[axis].min(body.lo[axis]);
            hi[axis] = hi[axis].max(body.hi[axis]);
        }
    }
    let whole = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let mut wanted: Vec<String> = bodies
        .iter()
        .map(|body| describe(body, whole, lo, hi).unwrap_or_else(|| "unnamed".to_owned()))
        .collect();
    // Panels facing front to back are decided together: the ones standing at an outer face are that panel, what stands between
    // them divides the space.
    let mut standing: Vec<usize> = (0..bodies.len())
        .filter(|index| matches!(wanted[*index].as_str(), "divider" | "panel - rear" | "panel - front"))
        .collect();
    if standing.len() >= 4 {
        standing.sort_by(|a, b| bodies[*a].centre()[1].total_cmp(&bodies[*b].centre()[1]));
        let reach = whole[1] * 0.05;
        let front_edge = bodies[standing[0]].centre()[1];
        let back_edge = bodies[*standing.last().expect("standing")].centre()[1];
        for index in standing.iter() {
            let depth = bodies[*index].centre()[1];
            wanted[*index] = if depth - front_edge <= reach {
                "panel - front".to_owned()
            } else if back_edge - depth <= reach {
                "panel - rear".to_owned()
            } else {
                "divider".to_owned()
            };
        }
    }
    let mut used: Vec<String> = Vec::new();
    let mut named = 0usize;
    let mut unnamed = 0usize;
    for (body, wanted) in bodies.iter().zip(wanted.into_iter()) {
        let mut name = wanted.clone();
        let mut repeat = 2;
        while wanted != "unnamed" && used.contains(&name) {
            name = format!("{wanted} ({repeat})");
            repeat += 1;
        }
        used.push(name.clone());
        if name == "unnamed" {
            unnamed += 1;
            continue;
        }
        if name == body.name {
            continue;
        }
        let done = app
            .session
            .execute("body.rename", &json!({ "body": body.name, "name": name }))
            .is_ok();
        if done {
            named += 1;
        } else {
            unnamed += 1;
        }
    }
    (named, unnamed)
}
