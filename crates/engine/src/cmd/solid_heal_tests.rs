//! `solid.heal`: the seam two coplanar faces leave is dissolved, and a body that has none is
//! left exactly as it was (counts unchanged, geometry unchanged).

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// The kernel's faces and edges of a body, and its measured bounding box.
fn shape(s: &mut Session, body: &str) -> (usize, usize, Vec<f64>) {
    let measured = run(s, "inspect.measure", json!({}));
    let bbox = measured["bodies"]
        .as_array()
        .and_then(|list| list.iter().find(|b| b["name"] == body))
        .map(|b| {
            let mut v: Vec<f64> = b["bbox"]["min"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            v.extend(b["bbox"]["max"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()));
            v
        })
        .unwrap_or_default();
    let st = s.model.state();
    let mb = st.body(body).unwrap_or_else(|| panic!("no body `{body}`"));
    (mb.body.face_count(), mb.body.edge_count(), bbox)
}

/// A box whose top face a Split Face cut in two: two coplanar faces sharing an edge, which is the
/// seam a drawing's split (or a join that could not sew it) leaves behind.
fn split_box(s: &mut Session) {
    run(s, "solid.box", json!({"length": 40, "width": 30, "height": 20}));
    // A plane across the box's middle: XZ sits on its bottom edge, which crosses nothing.
    run(s, "solid.split_face", json!({"faces": [[20, 15, 20]], "plane": {"origin": [0, 15, 0], "normal": [0, 1, 0]}}));
}

#[test]
fn solid_heal_dissolves_the_seam_a_split_left() {
    let mut s = Session::default();
    split_box(&mut s);
    let (faces, edges, bbox) = shape(&mut s, "Body1");
    assert_eq!(faces, 7, "the split leaves two coplanar pieces");
    let out = run(&mut s, "solid.heal", json!({"body": "Body1"}));
    assert_eq!(out["faces_before"], json!(faces), "{out}");
    assert_eq!(out["faces_after"], json!(faces - 1), "{out}");
    assert_eq!(out["faces_dissolved"], json!(1), "{out}");
    assert!(out["edges_after"].as_u64().unwrap() < edges as u64, "{out}");
    assert_eq!(out["edges_dissolved"].as_u64().unwrap(), edges as u64 - out["edges_after"].as_u64().unwrap(), "{out}");
    let (faces_after, _, bbox_after) = shape(&mut s, "Body1");
    assert_eq!(faces_after, faces - 1);
    for (a, b) in bbox.iter().zip(&bbox_after) {
        assert!((a - b).abs() < 1e-9, "{bbox:?} vs {bbox_after:?}");
    }
}

/// A join of two bodies on a whole face leaves no seam: the kernel heals every boolean, so a heal
/// after it reports nothing dissolved and the body measures the same.
#[test]
fn solid_heal_finds_nothing_on_a_body_a_join_already_healed() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Panel"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [40, 0, 0], "body_name": "Next"}));
    run(&mut s, "solid.combine", json!({"target": "Panel", "tools": ["Next"], "operation": "join"}));
    let (faces, edges, bbox) = shape(&mut s, "Panel");
    assert_eq!((faces, edges), (6, 12), "the join is one box already");
    let out = run(&mut s, "solid.heal", json!({"body": "Panel"}));
    assert_eq!(out["faces_dissolved"], json!(0), "{out}");
    assert_eq!(out["edges_dissolved"], json!(0), "{out}");
    let (faces_after, edges_after, bbox_after) = shape(&mut s, "Panel");
    assert_eq!((faces_after, edges_after), (faces, edges));
    assert_eq!(bbox, bbox_after);
}

/// A body that does not exist is refused before anything runs; the body name is required.
#[test]
fn solid_heal_names_the_body_it_cannot_find() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let err = s.execute("solid.heal", &json!({"body": "Nope"})).unwrap_err().to_string();
    assert!(err.contains("no body `Nope`"), "{err}");
    assert!(s.execute("solid.heal", &json!({})).is_err());
}
