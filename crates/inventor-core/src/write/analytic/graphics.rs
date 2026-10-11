use super::*;
use serde_json::json;
const OWNER: &str = "PmGraphicsSegment";
const GROUP: &str = "a79eaccf-11d1-c281-6000-a38ab46bceb0";
const FEATURE: &str = "0de8e459-4c26-c35a-3711-178d7c4b9b01";
const MESH: &str = "a79eacd2-11d1-c281-6000-a38ab46bceb0";
const COORD: &str = "a79eaccb-11d1-c281-6000-a38ab46bceb0";
const ENTITY: &str = "5194e9a3-11d2-289b-6000-6ab72c39cdb0";
const PLANE: &str = "37db9d1e-11d4-38af-6000-b1b7b035f1b0";
const CYLINDER: &str = "03e3d90b-4d6e-2fcd-5b14-79b35304793b";
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaceDisplay {
    pub bounds_mm: [f64; 6],
    pub vertices_mm: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub uv: Vec<[f64; 2]>,
    pub triangles: Vec<[u32; 3]>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeDisplay {
    pub bounds_mm: [f64; 6],
    pub points_mm: Vec<[f64; 3]>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Display {
    pub bounds_mm: [f64; 6],
    pub faces: Vec<FaceDisplay>,
    pub edges: Vec<EdgeDisplay>,
}
fn finite(x: f64) -> bool {
    x.is_finite() && x.abs() <= 1e7
}
fn bounded(p: &[f64; 3], b: &[f64; 6]) -> bool {
    (0..3).all(|i| finite(p[i]) && p[i] >= b[i] - 0.02 && p[i] <= b[i + 3] + 0.02)
}
fn valid_bounds(b: &[f64; 6]) -> bool {
    b.iter().all(|&x| finite(x)) && (0..3).all(|i| b[i] <= b[i + 3])
}
impl Display {
    pub(super) fn validate(&self, solid: &Solid) -> Result<()> {
        if !valid_bounds(&self.bounds_mm)
            || (0..3).any(|i| self.bounds_mm[i] >= self.bounds_mm[i + 3])
            || self.faces.len() != solid.faces.len()
            || self.edges.len() != solid.edges.len()
        {
            return Err(Error("display topology/bounds differs".into()));
        }
        let mut budget = 0usize;
        for f in &self.faces {
            let n = f.vertices_mm.len();
            budget += n + f.triangles.len();
            if !(3..=200000).contains(&n)
                || f.triangles.is_empty()
                || f.triangles.len() > 200000
                || f.normals.len() != n
                || f.uv.len() != n
                || !valid_bounds(&f.bounds_mm)
                || f.vertices_mm
                    .iter()
                    .any(|p| !bounded(p, &f.bounds_mm) || !bounded(p, &self.bounds_mm))
                || f.normals.iter().any(|p| {
                    p.iter().any(|&x| !finite(x))
                        || (p.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() > 1e-8
                })
                || f.uv.iter().flatten().any(|&x| !finite(x))
                || f.triangles.iter().any(|t| {
                    t.iter().any(|&i| i as usize >= n)
                        || t[0] == t[1]
                        || t[1] == t[2]
                        || t[0] == t[2]
                })
            {
                return Err(Error("invalid display mesh/budget".into()));
            }
        }
        for e in &self.edges {
            budget += e.points_mm.len();
            if !(2..=200000).contains(&e.points_mm.len())
                || !valid_bounds(&e.bounds_mm)
                || e.points_mm
                    .iter()
                    .any(|p| !bounded(p, &e.bounds_mm) || !bounded(p, &self.bounds_mm))
            {
                return Err(Error("invalid display edge/budget".into()));
            }
        }
        if budget > 1000000
            || solid
                .vertices_mm
                .iter()
                .any(|p| !bounded(p, &self.bounds_mm))
        {
            return Err(Error(
                "display aggregate budget or solid bounds differs".into(),
            ));
        }
        Ok(())
    }
}
fn role(class: &str, key: usize) -> String {
    format!("{class}/key:{key}")
}
fn fixed(name: &str, kind: &str, value: impl Into<Value>) -> Value {
    json!({"name":name,"kind":kind,"policy":"fixed","value":value.into()})
}
fn local(name: &str, role: &str, class: &str, bare: bool) -> Value {
    json!({"name":name,"kind":"u32","policy":"local_reference","namespace":if bare {"local_bare"} else {"local_qualified"},"target_role":role,"target_class":class})
}
fn header() -> Vec<Value> {
    vec![
        fixed("header_state", "u32", 0),
        json!({"name":"assigned_key","kind":"u16","policy":"assigned_key"}),
    ]
}
fn words<T: Into<Value>>(
    fields: &mut Vec<Value>,
    name: &str,
    kind: &str,
    values: impl IntoIterator<Item = T>,
) {
    fields.extend(
        values
            .into_iter()
            .enumerate()
            .map(|(i, v)| fixed(&format!("{name}_{i}"), kind, v)),
    );
}
fn array<T: Into<Value>>(
    fields: &mut Vec<Value>,
    name: &str,
    kind: &str,
    values: Vec<T>,
    width: usize,
    frame: u32,
) {
    let n = (values.len() / width) as u32;
    words(
        fields,
        &format!("{name}_header"),
        "u32",
        [0x30000002, n, n, frame],
    );
    words(fields, name, kind, values);
}
fn record(class: &str, role: &str, fields: Vec<Value>, keyed: bool) -> Value {
    json!({"owner":OWNER,"role":role,"class":class,"generation_contract":"graphics-fixed","keyed":keyed,"recipe":{"fields":fields}})
}
fn primitive(owner: &str) -> Vec<Value> {
    let mut f = header();
    words(&mut f, "primitive_flags", "u32", [0, 0, 0]);
    f.push(local("primitive_owner", owner, ENTITY, false));
    f.push(fixed("primitive_word22", "u32", 0));
    f
}
pub(super) fn construct(solid: &Solid, display: &Display) -> Result<profile::Profile> {
    let mut p = profile::Profile::load()?;
    let records = p.0["records"]
        .as_array_mut()
        .ok_or_else(|| Error("profile records absent".into()))?;
    records.retain(|r| {
        !(r["owner"] == OWNER
            && (r["generation_contract"] == "graphics-entity"
                || r["class"] == PLANE
                || matches!(
                    r["recipe"]["geometry_role"]["domain"].as_str(),
                    Some("face" | "edge")
                )))
    });
    if records.iter().filter(|r| r["owner"] == OWNER).count() != 227 {
        return Err(Error("invariant Graphics census differs".into()));
    }
    let nf = solid.faces.len();
    let n = nf + solid.edges.len();
    let entities: Vec<_> = (0..n).map(|i| role(ENTITY, 202 + i)).collect();
    for r in records.iter_mut() {
        if r["owner"] != OWNER {
            continue;
        }
        let targets = if r["role"] == role(GROUP, 4) {
            &entities[..nf]
        } else if r["role"] == role(GROUP, 5) {
            &entities[nf..]
        } else {
            continue;
        };
        let f = r["recipe"]["fields"]
            .as_array_mut()
            .ok_or_else(|| Error("group fields absent".into()))?;
        let end = f
            .iter()
            .position(|v| v["name"] == "draw_members_marker")
            .ok_or_else(|| Error("group members absent".into()))?;
        f.truncate(end);
        words(
            f,
            "draw_members",
            "u32",
            [0x30000002, targets.len() as u32, targets.len() as u32, 16],
        );
        f.extend(
            targets
                .iter()
                .enumerate()
                .map(|(i, t)| local(&format!("member_{i}"), t, ENTITY, false)),
        );
        f.push(fixed("terminal", "u8", 0));
    }
    let (mut wrappers, mut bindings, mut draws) = (vec![], vec![], vec![]);
    let mut cylinders = 0;
    for (i, er) in entities.iter().enumerate() {
        let face = i < nf;
        let cylinder = face && matches!(solid.faces[i].surface, Surface::Cylinder { .. });
        let frame = if face {
            if cylinder {
                0x46400000u32
            } else {
                0x45400000
            }
        } else if matches!(solid.edges[i - nf].curve, Curve::Line { .. }) {
            0x51000000
        } else {
            0x52000000
        };
        let class = if face { MESH } else { COORD };
        let pr = role(class, 202 + n + i);
        let br = format!("{er}/primitive_header_word_unknown_2");
        let bc = if cylinder { CYLINDER } else { PLANE };
        let mut f = header();
        words(&mut f, "primitive_flags", "u32", [frame, 0]);
        f.push(if face {
            local("binding", &br, bc, true)
        } else {
            fixed("binding", "u32", 0)
        });
        f.push(local(
            "owner",
            &role(GROUP, if face { 4 } else { 5 }),
            GROUP,
            false,
        ));
        f.push(fixed("word22", "u32", 0));
        words(&mut f, "lod", "u32", [0x30000002, 2, 2, 16]);
        f.push(local("draw0", &pr, class, false));
        f.push(fixed("draw1", "u32", 0));
        f.push(fixed("terminal", "u8", 0));
        let bounds = if face {
            display.faces[i].bounds_mm
        } else {
            display.edges[i - nf].bounds_mm
        };
        words(&mut f, "bounds_cm", "f64", bounds.map(|x| x / 10.));
        words(&mut f, "entity", "u32", [i as u32 + 1, 0, 0]);
        wrappers.push(record(ENTITY, er, f, true));
        if face {
            let mut f = vec![local("draw", er, ENTITY, false)];
            words(&mut f, "members", "u32", [0x30000002, 1, 1, 0x10000000]);
            f.push(local("feature", &role(FEATURE, 60), FEATURE, true));
            words(&mut f, "binding_tail", "u32", [0xffffffffu32, 0]);
            f.push(fixed("zero", "u8", 0));
            if let Surface::Cylinder { center, axis, .. } = solid.faces[i].surface {
                words(&mut f, "origin_cm", "f64", center.map(|x| x / 10.));
                words(&mut f, "axis", "f64", axis);
                cylinders += 1;
            }
            bindings.push(record(bc, &br, f, false));
            let d = &display.faces[i];
            let mut f = primitive(er);
            array(
                &mut f,
                "vertices",
                "f32",
                d.vertices_mm.iter().flatten().map(|x| x / 10.).collect(),
                3,
                278,
            );
            array(
                &mut f,
                "indices",
                "u32",
                d.triangles.iter().flatten().copied().collect(),
                1,
                8,
            );
            array(
                &mut f,
                "normals",
                "f32",
                d.normals.iter().flatten().copied().collect(),
                3,
                279,
            );
            array(
                &mut f,
                "uv",
                "f32",
                d.uv.iter()
                    .flat_map(|uv| [uv[0] / if cylinder { 1. } else { 10. }, uv[1] / 10.])
                    .collect(),
                2,
                296,
            );
            words(&mut f, "mesh_tail", "u32", [259, 0x30000002, 0]);
            words(&mut f, "mesh_range", "f32", [-0.1, 0.1]);
            draws.push(record(MESH, &pr, f, true));
        } else {
            let mut f = primitive(er);
            array(
                &mut f,
                "coordinates",
                "f32",
                display.edges[i - nf]
                    .points_mm
                    .iter()
                    .flatten()
                    .map(|x| x / 10.)
                    .collect(),
                3,
                278,
            );
            f.push(fixed("terminal", "u8", 0));
            draws.push(record(COORD, &pr, f, true));
        }
    }
    records.extend(wrappers);
    records.extend(bindings);
    records.extend(draws);
    if cylinders > 0 {
        let owner = p.0["owners"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|o| o["name"] == OWNER)
            .unwrap();
        let classes = owner["classes"].as_array_mut().unwrap();
        classes.push(json!({"index":classes.len(),"guid":CYLINDER,"word0":0,"word2":0,"word3":cylinders,"dependency_count":1,"dependencies":[PLANE]}));
        owner["dependency_order"]
            .as_array_mut()
            .unwrap()
            .push(CYLINDER.into());
    }
    Ok(p)
}
