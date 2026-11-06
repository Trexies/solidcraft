//! Where the panels of a drawing meet, and which of those seams take a cam lock.
//!
//! **Dead for `Calc Camlocks`.** The button now runs `ledger-holes`, the vendor hole engine Ledger ships, and places one
//! block per bore that engine returns (`crate::ledger_cams`). This module is kept, not deleted, because its seam model is
//! still the readable statement of how a box cabinet's panels meet and the tests beside it still describe the joints;
//! nothing on the button's path calls it, so a count from here is no longer a count the editor shows.
//!
//! This is Ledger's `src/joints.rs` translated into the editor, so the `Calc Camlocks` button marks the same bores Ledger's
//! parts window marks. The module is a copy on purpose: the seam model is the shop's, and a rewrite of it would drift from
//! the program the shop measures against.
//!
//! A board is a box. Its **thickness axis** is its thinnest dimension, so the faces across that axis are its broad faces and
//! the faces along it are its edges. A joint is read from which of those meet:
//!
//! | one panel presents | the other presents | what it is | hardware |
//! |---|---|---|---|
//! | a broad face | an edge | edge-to-face (a shelf into a side) | cam bores along the seam |
//! | an edge | an edge | edge-to-edge (a corner) | the same |
//! | a broad face | a broad face | stacked | none: two faces lying on each other are not a joint |
//!
//! The rules are the shop's, copied from Ledger's `migrations/0010_drill_rules.sql`: the editor has no database, so the
//! carcass buckets and the hinge heights are written down here once, with the migration named beside them.
//!
//! What is not here is Ledger's `Tally` and its door guess. `automatic` there also totals dowels for the parts list and
//! guesses which panels are hung on hinges rather than jointed in; neither can add or remove a cam seat, and this button
//! only marks bores.

/// How close two panels have to be to count as touching, in millimetres. Boards are modelled flush; this is the model's own
/// rounding, not a shop tolerance.
pub const TOUCH_MM: f64 = 1.0;

/// The boards a cam joint is drilled into: thinner than a countertop, thicker than a back panel. A metal leg, a 40 mm top or
/// a 6 mm back cannot take a 15 mm cam bore, so a seam involving one carries no cam hardware however cleanly it lines up.
pub const BOARD_MIN_MM: f64 = 12.0;
pub const BOARD_MAX_MM: f64 = 30.0;

/// A board, as the model measured it: the two corners of its box.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub name: String,
    pub lo: [f64; 3],
    pub hi: [f64; 3],
}

impl Panel {
    pub fn new(name: &str, lo: [f64; 3], hi: [f64; 3]) -> Self {
        Panel { name: name.to_owned(), lo, hi }
    }

    fn size(&self, axis: usize) -> f64 {
        (self.hi[axis] - self.lo[axis]).abs()
    }

    /// The axis the board is thinnest along: its broad faces face this way.
    pub fn thickness_axis(&self) -> usize {
        let mut thinnest = 0;
        for axis in 1..3 {
            if self.size(axis) < self.size(thinnest) {
                thinnest = axis;
            }
        }
        thinnest
    }
}

/// Whether a panel is a board a cam can go into.
pub fn is_board(panel: &Panel) -> bool {
    let thickness = panel.size(panel.thickness_axis());
    (BOARD_MIN_MM..=BOARD_MAX_MM).contains(&thickness)
}

/// How two panels meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One panel's edge lies in the other's broad face: the joint a shelf makes with a side.
    EdgeToFace,
    /// Two edges butt: a corner.
    EdgeToEdge,
    /// Two broad faces lying on each other: stacked boards, not a joint.
    Stacked,
}

/// One seam between two panels.
#[derive(Debug, Clone, PartialEq)]
pub struct Contact {
    /// Index of the first panel and of the second, in the order given to [`contacts`].
    pub a: usize,
    pub b: usize,
    pub kind: Kind,
    /// How long the seam runs, along the shared direction - the length the hardware has to cover.
    pub run: f64,
}

/// Every seam in a drawing: for each pair of panels, the one axis they meet on, with the longest run of that meeting.
///
/// A pair of panels can be flush on more than one axis (a corner). Only the longest run is kept: the other axis is the same
/// joint seen end-on, and counting it twice would drill the seam twice.
pub fn contacts(panels: &[Panel], tolerance: f64) -> Vec<Contact> {
    let mut found: Vec<Contact> = Vec::new();
    for a in 0..panels.len() {
        for b in (a + 1)..panels.len() {
            let (left, right) = (&panels[a], &panels[b]);
            let mut best: Option<Contact> = None;
            for axis in 0..3 {
                let touching = (left.hi[axis] - right.lo[axis]).abs() <= tolerance || (right.hi[axis] - left.lo[axis]).abs() <= tolerance;
                if !touching {
                    continue;
                }
                // The seam only exists where the panels overlap across the other two axes.
                let overlaps: Vec<f64> = (0..3)
                    .filter(|other| *other != axis)
                    .map(|other| left.hi[other].min(right.hi[other]) - left.lo[other].max(right.lo[other]))
                    .collect();
                if overlaps.iter().any(|overlap| *overlap <= tolerance) {
                    continue;
                }
                let run = overlaps.iter().cloned().fold(0.0, f64::max);
                let (a_broad, b_broad) = (left.thickness_axis() == axis, right.thickness_axis() == axis);
                let kind = if a_broad && b_broad {
                    Kind::Stacked
                } else if a_broad || b_broad {
                    Kind::EdgeToFace
                } else {
                    Kind::EdgeToEdge
                };
                let candidate = Contact { a, b, kind, run };
                if best.as_ref().map_or(true, |seen| candidate.run > seen.run) {
                    best = Some(candidate);
                }
            }
            if let Some(contact) = best {
                found.push(contact);
            }
        }
    }
    found
}

/// Where one bore goes: the point at the seam, the way the bore runs, and the size the shop drills it.
#[derive(Debug, Clone, PartialEq)]
pub struct Seat {
    /// `cam` or `dowel`.
    pub kind: &'static str,
    pub at: [f64; 3],
    /// The seam's own axis: the direction the two panels meet along.
    pub axis: [f64; 3],
    pub dia: f64,
    pub depth: f64,
}

/// What a drawing needs, read from its seams alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Automatic {
    /// How many seams carried hardware.
    pub seams: usize,
    /// Every bore the count came from, so the shop can see where they land.
    pub seats: Vec<Seat>,
}

/// One bucket of a joint's length table: for a seam up to `max_len` mm, `cams` cam bores the first `first` mm from the end,
/// the rest every `pitch`, and the last pinned `last` mm from the far end when `last_locked`.
#[derive(Debug, Clone, PartialEq)]
pub struct LengthRule {
    pub max_len: f64,
    pub cams: u32,
    pub dowels: u32,
    pub pitch: f64,
    pub first: f64,
    pub last: f64,
    /// The shop's row carries it; this pass reads neither end's first lock nor the edge offset.
    #[allow(dead_code)]
    pub first_locked: bool,
    pub last_locked: bool,
    #[allow(dead_code)]
    pub side_offset: f64,
}

/// The rules a job is drilled by, as they come out of the shop's own tables.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rules {
    /// `(kind, rule)` with `kind` equal to `carcass` or `back`, in the order the shop wrote them.
    pub joints: Vec<(&'static str, LengthRule)>,
}

impl Rules {
    /// The buckets of one joint kind, in the shop's order.
    pub fn of_kind(&self, kind: &str) -> Vec<&LengthRule> {
        self.joints.iter().filter(|(k, _)| *k == kind).map(|(_, rule)| rule).collect()
    }

    /// The shop's own rows, as `migrations/0010_drill_rules.sql` seeds them: three carcass buckets and two for a back.
    /// The carcass rows are the seam table a jointed edge is drilled by - two bores, 20 mm from the near end and 40 mm from
    /// the far one - and they are what a 48 x 72 table's seams were counted with.
    pub fn shop() -> Self {
        let carcass = |max_len: f64, first_locked: bool| LengthRule {
            max_len,
            cams: 2,
            dowels: 0,
            pitch: 32.0,
            first: 20.0,
            last: 40.0,
            first_locked,
            last_locked: true,
            side_offset: 9.0,
        };
        Rules {
            joints: vec![
                ("carcass", carcass(80.0, false)),
                ("carcass", carcass(100.0, true)),
                ("carcass", carcass(2440.0, false)),
                ("back", carcass(100.0, false)),
                ("back", carcass(2440.0, false)),
            ],
        }
    }
}

/// The rule for a seam of `len` mm: the smallest bound that still covers it, or the largest there is.
pub fn pick_rule<'a>(rules: &[&'a LengthRule], len: f64) -> Option<&'a LengthRule> {
    let mut sorted: Vec<&&LengthRule> = rules.iter().collect();
    sorted.sort_by(|a, b| a.max_len.total_cmp(&b.max_len));
    sorted.iter().find(|rule| len <= rule.max_len).or_else(|| sorted.last()).map(|rule| **rule)
}

/// Where the cam bores sit along a jointed edge of `len` mm.
pub fn cam_positions(rule: &LengthRule, len: f64) -> Vec<f64> {
    if rule.cams == 0 || len <= 0.0 {
        return Vec::new();
    }
    let usable_end = len - rule.last;
    if usable_end <= rule.first {
        // Too short for both margins: spread them rather than stacking bores on one spot.
        return (1..=rule.cams).map(|i| len * f64::from(i) / f64::from(rule.cams + 1)).collect();
    }
    let mut xs: Vec<f64> =
        (0..rule.cams).map(|i| if rule.last_locked && i + 1 == rule.cams { usable_end } else { rule.first + f64::from(i) * rule.pitch }).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    xs.retain(|x| *x >= 0.0 && *x <= len);
    xs
}

/// Where the dowels sit: half a pitch after each cam bore, as the old client interleaved them.
pub fn dowel_positions(rule: &LengthRule, len: f64) -> Vec<f64> {
    (0..rule.dowels).map(|i| rule.first + (f64::from(i) + 0.5) * rule.pitch).filter(|x| *x < len).collect()
}

/// The hardware a drawing needs, from where its faces meet: one run of cam bores and dowels per jointed seam.
pub fn automatic(panels: &[Panel], rules: &Rules) -> Automatic {
    let seams = contacts(panels, TOUCH_MM);
    let mut seats: Vec<Seat> = Vec::new();
    let mut jointed = 0;

    for contact in &seams {
        if contact.kind == Kind::Stacked {
            continue;
        }
        // Both panels have to be board: a joint into a steel leg is not a cam lock.
        if !(is_board(&panels[contact.a]) && is_board(&panels[contact.b])) {
            continue;
        }
        // One run of bores along the seam, by the same length buckets the old client drilled a jointed edge with.
        let buckets = rules.of_kind("carcass");
        let Some(rule) = pick_rule(&buckets, contact.run) else {
            continue;
        };
        // Where the seam actually is, so each bore can be marked: the axis the panels touch on, the axis the run goes along,
        // and the third one across the middle of the overlap.
        let (left, right) = (&panels[contact.a], &panels[contact.b]);
        let mut joint = 0;
        let mut gap = f64::INFINITY;
        for axis in 0..3 {
            let touching = (left.hi[axis] - right.lo[axis]).abs().min((right.hi[axis] - left.lo[axis]).abs());
            if touching < gap {
                gap = touching;
                joint = axis;
            }
        }
        let mut along = (joint + 1) % 3;
        let mut across = (joint + 2) % 3;
        let span = |axis: usize| {
            let overlap = left.hi[axis].min(right.hi[axis]) - left.lo[axis].max(right.lo[axis]);
            (overlap, axis)
        };
        if span(along).0 < span(across).0 {
            std::mem::swap(&mut along, &mut across);
        }
        let middle = (left.lo[across].max(right.lo[across]) + left.hi[across].min(right.hi[across])) / 2.0;
        let seam = (left.hi[joint] + right.lo[joint]) / 2.0;
        let mut axis = [0.0; 3];
        axis[joint] = 1.0;
        let mut seat = |x: f64, kind: &'static str, dia: f64, depth: f64| {
            let mut at = [0.0; 3];
            at[along] = left.lo[along].max(right.lo[along]) + x;
            at[across] = middle;
            at[joint] = seam;
            seats.push(Seat { kind, at, axis, dia, depth });
        };
        for x in cam_positions(rule, contact.run) {
            seat(x, "cam", 15.0, 14.2);
        }
        for x in dowel_positions(rule, contact.run) {
            seat(x, "dowel", 8.0, 12.0);
        }
        jointed += 1;
    }

    Automatic { seams: jointed, seats }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shelf whose edge lies against a side's broad face is one edge-to-face joint, as long as the shelf is deep - and it
    /// carries the shop's two cam bores, 20 mm in from the near end and 40 mm from the far one.
    #[test]
    fn a_shelf_edge_in_a_side_face_is_one_joint_with_two_cams() {
        let side = Panel::new("Side", [0.0, 0.0, 0.0], [18.0, 720.0, 450.0]);
        let shelf = Panel::new("Shelf", [18.0, 300.0, 0.0], [700.0, 318.0, 450.0]);
        let panels = vec![side, shelf];
        let seams = contacts(&panels, TOUCH_MM);
        assert_eq!(seams.len(), 1, "{seams:?}");
        assert_eq!(seams[0].kind, Kind::EdgeToFace);
        assert!((seams[0].run - 450.0).abs() < 1e-9, "the seam runs across the shelf's depth: {seams:?}");
        let auto = automatic(&panels, &Rules::shop());
        assert_eq!(auto.seats.len(), 2, "one run, two bores: {auto:?}");
        assert_eq!(auto.seams, 1);
    }

    /// Two boards whose broad faces lie on each other are stacked, not jointed: nothing is drilled.
    #[test]
    fn two_stacked_boards_need_nothing() {
        let lower = Panel::new("Lower", [0.0, 0.0, 0.0], [700.0, 450.0, 18.0]);
        let upper = Panel::new("Upper", [0.0, 0.0, 18.0], [700.0, 450.0, 36.0]);
        let panels = vec![lower, upper];
        assert_eq!(contacts(&panels, TOUCH_MM)[0].kind, Kind::Stacked);
        assert!(automatic(&panels, &Rules::shop()).seats.is_empty(), "stacked is not a joint");
    }

    /// Panels that do not touch are not a joint, however close they are in the list, and a seam longer than the table's last
    /// bucket still takes that bucket rather than nothing.
    #[test]
    fn panels_apart_are_not_joined() {
        let left = Panel::new("Left", [0.0, 0.0, 0.0], [18.0, 720.0, 450.0]);
        let right = Panel::new("Right", [600.0, 0.0, 0.0], [618.0, 720.0, 450.0]);
        assert!(contacts(&[left, right], TOUCH_MM).is_empty());
        let long = Panel::new("Top", [0.0, 0.0, 0.0], [18.0, 2440.0, 450.0]);
        let side = Panel::new("Side", [18.0, 0.0, 0.0], [700.0, 2440.0, 18.0]);
        assert_eq!(automatic(&[long, side], &Rules::shop()).seats.len(), 2, "past 2440 the longest bucket answers");
    }
}
