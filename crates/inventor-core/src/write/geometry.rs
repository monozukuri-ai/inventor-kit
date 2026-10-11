use super::document::Graph;
use super::*;
const BITS: [[usize; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [1, 1, 1],
    [0, 1, 1],
];
const WALKS: [[usize; 4]; 6] = [
    [0, 3, 2, 1],
    [4, 5, 6, 7],
    [0, 4, 7, 3],
    [1, 2, 6, 5],
    [0, 1, 5, 4],
    [3, 7, 6, 2],
];
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn vertices(g: &Graph<'_>) -> [[f64; 3]; 8] {
    // Consume the immutable authored kernel geometry. Repeating the mm->cm
    // arithmetic here can round differently from the SAB builder.
    let points: Vec<_> = g
        .shape
        .model()
        .entities()
        .iter()
        .filter_map(|entity| match entity {
            acis_core::entities::Entity::Point(p) => {
                Some([p.location.x, p.location.y, p.location.z])
            }
            _ => None,
        })
        .collect();
    let lo: [f64; 3] =
        std::array::from_fn(|i| points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min));
    let hi: [f64; 3] = std::array::from_fn(|i| {
        points
            .iter()
            .map(|p| p[i])
            .fold(f64::NEG_INFINITY, f64::max)
    });
    BITS.map(|bits| {
        *points
            .iter()
            .find(|p| (0..3).all(|i| p[i] == if bits[i] == 0 { lo[i] } else { hi[i] }))
            .expect("AuthoredBox validates all eight distinct corners")
    })
}
pub(super) fn bounds(g: &Graph<'_>) -> [f64; 6] {
    let p = vertices(g);
    [p[0][0], p[0][1], p[0][2], p[6][0], p[6][1], p[6][2]]
}
fn location(role: &Value) -> Result<[usize; 6]> {
    let a = array(role, "location")?;
    if a.len() != 6 {
        return Err(Error("invalid geometric location".into()));
    }
    let mut b = [0; 6];
    for (i, v) in a.iter().enumerate() {
        b[i] = number(v)? as usize;
        if b[i] > 1 {
            return Err(Error("invalid geometric corner bit".into()));
        }
    }
    Ok(b)
}
fn face_index(loc: [usize; 6]) -> Result<usize> {
    for (i, walk) in WALKS.iter().enumerate() {
        let min: Vec<_> = (0..3)
            .map(|axis| walk.iter().map(|&j| BITS[j][axis]).min().unwrap())
            .collect();
        let max: Vec<_> = (0..3)
            .map(|axis| walk.iter().map(|&j| BITS[j][axis]).max().unwrap())
            .collect();
        if (0..3).all(|axis| min[axis] == loc[axis] && max[axis] == loc[axis + 3]) {
            return Ok(i);
        }
    }
    Err(Error("location is not a box face".into()))
}
fn edges() -> Vec<(usize, usize)> {
    let mut set = std::collections::BTreeSet::new();
    for walk in WALKS {
        for i in 0..4 {
            let a = walk[i];
            let b = walk[(i + 1) % 4];
            set.insert((a.min(b), a.max(b)));
        }
    }
    set.into_iter().collect()
}
fn edge_index(loc: [usize; 6]) -> Result<(usize, usize, usize)> {
    for (i, (a, b)) in edges().into_iter().enumerate() {
        if (0..3).all(|axis| {
            BITS[a][axis].min(BITS[b][axis]) == loc[axis]
                && BITS[a][axis].max(BITS[b][axis]) == loc[axis + 3]
        }) {
            return Ok((i, a, b));
        }
    }
    Err(Error("location is not a box edge".into()))
}
fn rectangle(g: &Graph<'_>, slot: usize) -> Result<[f64; 6]> {
    let b = bounds(g);
    let pad: [f64; 3] = std::array::from_fn(|i| (b[i + 3] - b[i]) * 0.05);
    // Explicit authored display policy: include both origin and the translated body.
    let lo: [f64; 3] = std::array::from_fn(|i| b[i].min(0.0) - pad[i]);
    let hi: [f64; 3] = std::array::from_fn(|i| b[i + 3].max(0.0) + pad[i]);
    match slot {
        0 => Ok([lo[1], lo[2], 0., hi[1], hi[2], 0.]),
        1 => Ok([-hi[0], lo[2], 0., -lo[0], hi[2], 0.]),
        2 => Ok([lo[0], lo[1], 0., hi[0], hi[1], 0.]),
        _ => Err(Error("unknown workplane".into())),
    }
}
pub(super) fn field(g: &Graph<'_>, recipe: &Value, f: &Value) -> Result<Value> {
    let channel = string(f, "channel")?;
    let component = number(&f["component"])? as usize;
    let values: Vec<f64>;
    if let Some(axis) = recipe["axis"].as_str() {
        let (start, end) = match axis {
            "X" => (0.789999998435378, 3.2100000000745057),
            "Y" => (0.6849999889731406, 4.315000100433826),
            "Z" => (0.579999996870756, 5.4200000001490105),
            _ => return Err(Error("unknown axis".into())),
        };
        let cosine: f64 = -2.220446049250313e-16;
        let sine: f64 = 1.0000000000000002;
        let determinant = sine * sine + cosine * cosine;
        let coordinate = |p: f64| {
            let t =
                (p / (sine / determinant) - 1.0) * (cosine / determinant) + cosine / determinant;
            match axis {
                "X" => vec![p, 0., 0.],
                "Y" => vec![p, t, 0.],
                _ => vec![p, 0., t],
            }
        };
        let a = coordinate(start);
        let b = coordinate(end);
        values = match channel {
            "line" => [a, b].concat(),
            "start" => a,
            "end" => b,
            _ => return Err(Error("unknown axis channel".into())),
        };
    } else {
        let r = &recipe["geometry_role"];
        values = match string(r, "domain")? {
            "workplane" => {
                let b = rectangle(g, number(&r["slot"])? as usize)?;
                let corners = [
                    [b[0], b[1], 0.],
                    [b[0], b[4], 0.],
                    [b[3], b[4], 0.],
                    [b[3], b[1], 0.],
                ];
                match channel {
                    "rectangle" => b.to_vec(),
                    "vertices" => corners.concat(),
                    "indices" => vec![0., 3., 1., 1., 3., 2.],
                    "normals" => [0., 0., 1.].repeat(4),
                    "label" => vec![b[0], b[4], 0., b[3] - b[0], 0., 0.],
                    _ => {
                        if let Some(i) = channel.strip_prefix("corner-") {
                            corners
                                .get(i.parse::<usize>().map_err(err)?)
                                .ok_or_else(|| Error("unknown corner".into()))?
                                .to_vec()
                        } else if let Some(i) = channel.strip_prefix("border-") {
                            let i = i.parse::<usize>().map_err(err)?;
                            if i >= 4 {
                                return Err(Error("unknown border".into()));
                            }
                            [corners[i], corners[(i + 1) % 4]].concat()
                        } else {
                            return Err(Error("unknown workplane channel".into()));
                        }
                    }
                }
            }
            "edge" => {
                let (_, a, b) = edge_index(location(r)?)?;
                let p = vertices(g);
                let mut ends = [p[a], p[b]];
                ends.sort_by(|a, b| a.partial_cmp(b).unwrap());
                if channel != "coordinates" {
                    return Err(Error("unknown edge channel".into()));
                }
                ends.concat()
            }
            "face" => {
                let i = face_index(location(r)?)?;
                let all = vertices(g);
                let p = WALKS[i].map(|j| all[j]);
                let model = g.shape.model();
                let face = model
                    .entities()
                    .iter()
                    .filter_map(|entity| match entity {
                        acis_core::entities::Entity::Face(f) => Some(f),
                        _ => None,
                    })
                    .nth(i)
                    .ok_or_else(|| Error("authored face missing".into()))?;
                let surface = match &model.entities()[face.surface.0 as usize] {
                    acis_core::entities::Entity::PlaneSurface(s) => s,
                    _ => return Err(Error("authored face is not planar".into())),
                };
                let direction = [
                    surface.u_direction.x,
                    surface.u_direction.y,
                    surface.u_direction.z,
                ];
                let scale = dot(direction, direction).sqrt();
                let u = direction.map(|x| x / scale);
                let origin = [surface.origin.x, surface.origin.y, surface.origin.z];
                let normal = match i {
                    0 => [0., 0., -1.],
                    1 => [0., 0., 1.],
                    2 => [-1., 0., 0.],
                    3 => [1., 0., 0.],
                    4 => [0., -1., 0.],
                    _ => [0., 1., 0.],
                };
                let v = cross(normal, u);
                match channel {
                    "vertices" => p.concat(),
                    "indices" => {
                        if normal[0] != 0. {
                            vec![0., 1., 3., 3., 1., 2.]
                        } else {
                            vec![1., 2., 0., 0., 2., 3.]
                        }
                    }
                    "normals" => normal.to_vec().repeat(4),
                    "uv" => p
                        .iter()
                        .flat_map(|&p| {
                            let d = sub(p, origin);
                            [u, v].map(|axis| ((dot(d, axis) / scale) as f32 * scale as f32) as f64)
                        })
                        .collect(),
                    _ => return Err(Error("unknown face channel".into())),
                }
            }
            _ => return Err(Error("unknown geometric field domain".into())),
        };
    }
    let value = *values
        .get(component)
        .ok_or_else(|| Error("missing geometry component".into()))?;
    if string(f, "kind")? == "u32" {
        Ok((value as u32).into())
    } else {
        Ok(value.into())
    }
}
pub(super) fn custom(g: &Graph<'_>, r: &Value) -> Result<Vec<u8>> {
    let owner = string(r, "owner")?;
    let role = string(r, "role")?;
    let mut w = Wire::default();
    if string(r, "generation_contract")? == "graphics-curves" {
        let b = bounds(g);
        let b: Vec<_> = b
            .iter()
            .enumerate()
            .map(|(i, x)| x + if i < 3 { -1e-8 } else { 1e-8 })
            .collect();
        if string(r, "class")? == "a94779e0-11d4-3806-6000-b1b7b035f1b0" {
            w.words(&[
                g.reference(
                    owner,
                    "ca7163a3-11d0-d3b2-0008-bfbb21eddc09/key:2",
                    "local_qualified",
                )?,
                0,
                10,
            ]);
            for v in b {
                w.f64(v);
            }
            w.words(&[0, 0x30000002, 0]);
        } else if string(r, "class")? == "6a6931dc-46c9-c8bf-6e15-388ef67547f2" {
            w.words(&[9, 0, 0x30000002, 0]);
            for v in b {
                w.f64(v);
            }
        } else {
            return Err(Error("unknown Graphics curve cache".into()));
        }
    } else {
        let e = &r["entity"];
        let face = string(e, "kind")? == "planar-face";
        let loc = location(e)?;
        let id = if face {
            face_index(loc)? as u32 + 1
        } else {
            edge_index(loc)?.0 as u32 + 7
        };
        w.u32(0);
        w.u16(
            g.get(owner, role)?
                .key
                .ok_or_else(|| Error("draw key absent".into()))?,
        );
        let binding = if face {
            g.reference(owner, string(e, "binding")?, "local_bare")?
        } else {
            0
        };
        w.words(&[
            if face { 0x45400000 } else { 0x51000000 },
            0,
            binding,
            g.reference(owner, string(e, "owner")?, "local_qualified")?,
            0,
            0x30000002,
            2,
            2,
            0x10,
            g.reference(owner, string(e, "draw")?, "local_qualified")?,
            0,
        ]);
        w.u8(0);
        let b = bounds(g);
        for (i, bit) in loc.iter().enumerate() {
            let axis = i % 3;
            w.f64(if *bit == 0 { b[axis] } else { b[axis + 3] });
        }
        w.words(&[id, 0, 0]);
    }
    Ok(w.0)
}
