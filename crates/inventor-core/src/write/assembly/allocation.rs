//! Allocate the bounded flat-assembly graph from named roles, never IAM bytes.
use super::*;
use serde_json::{json, Value};

pub(super) const DC: &str = "AmDcSegment";
pub(super) const GR: &str = "AmGraphicsSegment";
pub(super) const BR: &str = "AmBrowserSegment";
pub(super) const RX: &str = "AmRxSegment";

fn layout(owner: &str, n: usize) -> Vec<(usize, Option<usize>)> {
    let mut out = Vec::new();
    let blocks: &[(usize, usize, bool)] = match owner {
        DC => &[(0, 8, false), (8, 9, true), (9, 21, false)],
        GR => &[
            (0, 10, false),
            (10, 12, true),
            (12, 32, false),
            (32, 35, true),
            (35, 114, false),
            (114, 116, true),
            (116, 170, false),
            (170, 173, true),
            (173, 204, false),
        ],
        BR => &[(0, 12, false), (12, 13, true), (13, 25, false)],
        RX => &[
            (0, 2, false),
            (2, 3, true),
            (3, 5, false),
            (5, 6, true),
            (6, 7, false),
            (7, 8, true),
            (8, 9, false),
            (9, 10, true),
        ],
        "AmAppSegment" => &[(0, 201, false)],
        "NBNotebookSegment" => &[(0, 2, false)],
        "AmBREPSegment" => &[(0, 6, false)],
        "DesignViewSegment" => &[(0, 4, false)],
        _ => unreachable!("embedded owner"),
    };
    let append = |out: &mut Vec<_>, a, b, repeat| {
        if repeat {
            for i in 0..n {
                for j in a..b {
                    out.push((j, Some(i)));
                }
            }
        } else {
            for j in a..b {
                out.push((j, None));
            }
        }
    };
    for &(a, b, repeat) in blocks {
        append(&mut out, a, b, repeat);
    }
    if owner == DC {
        out.extend([(21, Some(0)), (22, None)]);
        out.extend((1..n).map(|i| (21, Some(i))));
        for (a, b, repeat) in [
            (23, 25, false),
            (25, 26, true),
            (26, 28, true),
            (28, 36, false),
            (36, 37, true),
        ] {
            append(&mut out, a, b, repeat);
        }
    }
    out
}
fn role(index: usize, occurrence: Option<usize>) -> String {
    occurrence.map_or_else(|| format!("base/{index}"), |i| format!("occ/{i}/{index}"))
}
fn repeated(value: &Value, i: usize) -> Value {
    json!(value
        .as_str()
        .unwrap()
        .replace("occ/0/", &format!("occ/{i}/")))
}
fn key(owner: &str, index: usize, i: Option<usize>, n: usize, original: &Value) -> Value {
    let Some(i) = i else { return original.clone() };
    if original.is_null() {
        return Value::Null;
    }
    json!(match owner {
        DC => match index {
            26 | 27 => 36 + 4 * n + 2 * i + (index - 26),
            _ => {
                let first = match index {
                    8 => 36,
                    21 => 34,
                    25 => 37,
                    36 => 35,
                    _ => unreachable!(),
                };
                first + if i == 0 { 0 } else { 2 + 4 * i }
            }
        },
        GR =>
            (match index {
                10 => 160,
                11 => 162,
                33 => 161,
                _ => original.as_u64().unwrap() as usize,
            }) + 3 * i,
        BR => 112 + i,
        RX =>
            (match index {
                2 => 6,
                5 => 4,
                7 => 5,
                _ => original.as_u64().unwrap() as usize,
            }) + 3 * i,
        _ => unreachable!(),
    })
}
fn node_layout(owner: &str, n: usize, length: usize) -> Vec<(usize, Option<usize>)> {
    match owner {
        DC => (0..3)
            .map(|j| (j, None))
            .chain((3..5).flat_map(|j| (0..n).map(move |i| (j, Some(i)))))
            .collect(),
        GR => (0..8)
            .map(|j| (j, None))
            .chain((0..n).map(|i| (8, Some(i))))
            .collect(),
        BR => (0..3)
            .map(|j| (j, None))
            .chain((0..n).map(|i| (3, Some(i))))
            .chain((4..12).map(|j| (j, None)))
            .collect(),
        _ => (0..length).map(|j| (j, None)).collect(),
    }
}
fn capacity(initial: usize, count: usize, step: usize) -> usize {
    let mut value = initial;
    for required in ((initial + step)..(count + step)).step_by(step) {
        value = required.max(value + value / 2);
        if value >= count {
            break;
        }
    }
    value
}
fn fields(row: &mut Value) -> &mut Vec<Value> {
    row["recipe"]["fields"].as_array_mut().unwrap()
}
fn row<'a>(rows: &'a mut Value, role: &str) -> &'a mut Value {
    rows.as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["role"] == role)
        .unwrap()
}
fn set(row: &mut Value, name: &str, value: Value) {
    fields(row).iter_mut().find(|f| f["name"] == name).unwrap()["value"] = value;
}
fn coords(row: &mut Value, prefix: &str, values: &[f64]) {
    for (i, &v) in values.iter().enumerate() {
        set(row, &format!("{prefix}_{i}"), json!(v));
    }
}
fn members(rows: &mut Value, r: &str, name: &str, targets: Vec<String>, capacity: Option<usize>) {
    let classes: BTreeMap<String, Value> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["role"].as_str().unwrap().into(), r["class"].clone()))
        .collect();
    let fs = fields(row(rows, r));
    let start = fs
        .iter()
        .position(|f| f["name"] == format!("{name}_marker"))
        .unwrap();
    let old = fs[start + 1]["value"].as_u64().unwrap() as usize;
    assert!(old > 0);
    let template = fs[start + 4].clone();
    let mut replacement = fs[start..start + 4].to_vec();
    replacement[1]["value"] = json!(targets.len());
    replacement[2]["value"] = json!(capacity.unwrap_or(targets.len()));
    for (i, target) in targets.into_iter().enumerate() {
        let mut f = template.clone();
        f["name"] = json!(format!("{name}_{i}"));
        f["target_class"] = classes[&target].clone();
        f["target_role"] = json!(target);
        replacement.push(f);
    }
    fs.splice(start..start + 4 + old, replacement);
}
fn prefix_targets(seed: &Value, owner: &str, index: usize, name: &str, n: usize) -> Vec<String> {
    seed["graph"]["owners"][owner][index]["recipe"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| {
            f["policy"] == "local-reference" && f["name"].as_str().unwrap().starts_with(name)
        })
        .take(n)
        .map(|f| f["target_role"].as_str().unwrap().into())
        .collect()
}

fn graph(seed: &Value, doc: &PreparedAssembly) -> Value {
    let n = doc.occurrences.len();
    let original = &seed["graph"];
    let mut out = original.clone();
    out["occurrence_count"] = json!(n);
    out["reference_count"] = json!(doc.definitions.len());
    for (owner, nodes) in original["registry_nodes"].as_object().unwrap() {
        out["registry_nodes"][owner] =
            json!(node_layout(owner, n, nodes.as_array().unwrap().len())
                .into_iter()
                .map(|(j, i)| i.map_or_else(|| nodes[j].clone(), |i| repeated(&nodes[j], i)))
                .collect::<Vec<_>>());
    }
    for (owner, base) in original["owners"].as_object().unwrap() {
        let mut rows = Vec::new();
        for (index, occ) in layout(owner, n) {
            let mut r = base[index].clone();
            r["role"] = json!(role(index, occ));
            r["assigned_key"] = key(owner, index, occ, n, &r["assigned_key"]);
            if let Some(i) = occ {
                for f in fields(&mut r) {
                    if f["policy"] == "local-reference" {
                        f["target_role"] = repeated(&f["target_role"], i);
                    }
                    if f["policy"] == "registry-reference" {
                        f["target_node"] = repeated(&f["target_node"], i);
                    }
                    if f["policy"] == "document-state" {
                        f["state_role"] = json!(format!("placement-state/{}", i + 1));
                    }
                    if f["policy"] != "fixed" {
                        continue;
                    }
                    let name = f["name"].as_str().unwrap();
                    let plus = match (owner.as_str(), name) {
                        (DC, "document_object_id_candidate")
                        | (RX, "rx_transform_terminal_unknown") => {
                            if i == 0 {
                                0
                            } else {
                                1 + 2 * i
                            }
                        }
                        (DC, "label_document_object_id_candidate")
                        | (GR, "graphics_link_word_unknown_0" | "graphics_index_candidate")
                        | (BR, "tail_unknown_26") => i,
                        _ => 0,
                    };
                    if plus > 0 {
                        f["value"] = json!(f["value"].as_u64().unwrap() + plus as u64);
                    }
                    if matches!(
                        f["name"].as_str().unwrap(),
                        "occurrence_id"
                            | "graphics_occurrence_id_mirror"
                            | "placement_id_candidate"
                            | "rx_member_id_unknown"
                            | "rx_transform_flag1_unknown"
                    ) {
                        f["value"] = json!(i + 1);
                    }
                }
                if owner == RX && index == 2 && i > 0 {
                    fields(&mut r).retain(|f| f["name"] != "rx_prefix");
                }
            }
            rows.push(r);
        }
        out["owners"][owner] = json!(rows);
    }
    for owner in [DC, GR, BR, RX] {
        let rows = &mut out["owners"][owner];
        match owner {
            DC => {
                members(
                    rows,
                    "base/0",
                    "document_extra_members",
                    (0..n).map(|i| role(8, Some(i))).collect(),
                    None,
                );
                let t = (13..20)
                    .map(|j| role(j, None))
                    .chain((0..n).map(|i| role(8, Some(i))))
                    .chain([role(20, None)])
                    .collect();
                members(
                    rows,
                    "base/1",
                    "label_members",
                    t,
                    Some(capacity(11, 8 + n, 1)),
                );
                let mut t = prefix_targets(seed, DC, 2, "members_", 8);
                t.extend([role(21, Some(0)), role(8, Some(0)), role(22, None)]);
                t.extend((1..n).flat_map(|i| [role(21, Some(i)), role(8, Some(i))]));
                let cap = capacity(12, t.len(), 1);
                members(rows, "base/2", "members", t, Some(cap));
                members(
                    rows,
                    "base/12",
                    "context_members",
                    (0..n)
                        .flat_map(|i| [role(26, Some(i)), role(27, Some(i))])
                        .collect(),
                    Some(capacity(2, 2 * n, 2)),
                );
                set(
                    row(rows, "base/2"),
                    "header_flags_unknown",
                    json!(if n == 1 { 256 } else { 0 }),
                );
                set(
                    row(rows, "base/2"),
                    "root_word_unknown_3",
                    json!(10 + 2 * n),
                );
                set(row(rows, "base/2"), "root_word_unknown_4", json!(10 + n));
            }
            GR => {
                let mut t = prefix_targets(seed, GR, 0, "members_", 8);
                t.extend((0..n).flat_map(|i| [role(10, Some(i)), role(11, Some(i))]));
                members(
                    rows,
                    "base/0",
                    "members",
                    t,
                    Some(capacity(12, 8 + 2 * n, 1)),
                );
            }
            BR => {
                members(
                    rows,
                    "base/4",
                    "list",
                    (6..12)
                        .map(|j| role(j, None))
                        .chain((0..n).map(|i| role(12, Some(i))))
                        .collect(),
                    None,
                );
                set(row(rows, "base/0"), "terminal_unknown_word", json!(101 + n));
            }
            RX => members(
                rows,
                "base/0",
                "rx_members",
                (0..n).map(|i| role(2, Some(i))).collect(),
                None,
            ),
            _ => unreachable!(),
        }
    }
    let r = &mut out["owners"]["DesignViewSegment"][3];
    set(r, "view_state_slot_count_candidate", json!(7 + n));
    let fs = fields(r);
    let pos = fs
        .iter()
        .position(|f| f["name"] == "view_state_slot_count_candidate")
        .unwrap();
    fs[pos + 1]["value"] = json!(32771 + n);
    let tail = fs.last().unwrap().clone();
    for i in 8..7 + n {
        let mut f = tail.clone();
        f["name"] = json!(format!("view_state_packed_slot_unknown_{i}"));
        fs.push(f);
    }
    out
}
fn tables(seed: &Value, g: &Value, n: usize) -> Value {
    let mut out = seed["tables"].clone();
    for owner in out["owners"].as_array_mut().unwrap() {
        let name = owner["name"].as_str().unwrap().to_owned();
        let before = owner["nodes"].as_array().unwrap();
        let mut nodes = Vec::new();
        for (j, i) in node_layout(&name, n, before.len()) {
            let mut node = before[j].clone();
            let target = node["target_owner"].as_str().unwrap();
            let old = &seed["graph"]["owners"][target]
                [node["target_ordinal"].as_u64().unwrap() as usize]["role"];
            let target_role = i.map_or_else(|| old.clone(), |i| repeated(old, i));
            let (ordinal, record) = g["owners"][target]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .find(|(_, r)| r["role"] == target_role)
                .unwrap();
            assert_eq!(record["class"], node["class"]);
            node["key"] = record["assigned_key"].clone();
            node["target_ordinal"] = json!(ordinal);
            nodes.push(node);
        }
        owner["nodes"] = json!(nodes);
        let (allocator, state) = match name.as_str() {
            DC => (
                35 + 6 * n,
                vec![
                    0,
                    if n == 1 { 3 } else { 4 },
                    1,
                    31 + 6 * n,
                    31 + 6 * n,
                    4 + 2 * n,
                ],
            ),
            GR => (159 + 3 * n, vec![0, 5, 1, 194 + 10 * n, 69 + 3 * n, 9 + n]),
            BR => (111 + n, vec![0, 4, 1, 24 + n, 25 + n, 12 + n]),
            RX => (
                3 + 3 * n,
                vec![
                    0,
                    2,
                    1,
                    g["owners"][RX].as_array().unwrap().len(),
                    3 + 3 * n,
                    1,
                ],
            ),
            _ => continue,
        };
        let tail = owner["meta6"]["allocator"].as_u64().unwrap() as usize / 8 * 8;
        let old = owner["meta6"]["blocks"][0]["selected_ids"]
            .as_array()
            .unwrap();
        let selected: BTreeSet<usize> = old
            .iter()
            .map(|v| v.as_u64().unwrap() as usize)
            .filter(|&v| v < tail)
            .chain(tail..(allocator / 8 + 1) * 8)
            .collect();
        owner["meta6"]["blocks"][0]["selected_ids"] = json!(selected);
        owner["meta6"]["allocator"] = json!(allocator);
        owner["registry_state"] = json!(state);
    }
    out
}
fn drawing(g: &mut Value, b: &[f64; 6], seed: &Value) {
    let p = [
        (b[3] - b[0]) * 0.05,
        (b[4] - b[1]) * 0.05,
        (b[5] - b[2]) * 0.05,
    ];
    let rectangles = [
        (b[1] - p[1], b[2] - p[2], b[4] + p[1], b[5] + p[2]),
        (-b[3] - p[0], b[2] - p[2], -b[0] + p[0], b[5] + p[2]),
        (b[0] - p[0], b[1] - p[1], b[3] + p[0], b[4] + p[1]),
    ];
    let rows = &mut g["owners"][GR];
    for (plane, (x0, y0, x1, y1)) in rectangles.into_iter().enumerate() {
        let bounds = [x0, y0, 0., x1, y1, 0.];
        let corners = [[x0, y0, 0.], [x0, y1, 0.], [x1, y1, 0.], [x1, y0, 0.]];
        coords(
            row(rows, &role(2 + plane, None)),
            "plane_container_numbers",
            &bounds,
        );
        let offset = 14 * plane;
        coords(
            row(rows, &role(39 + offset, None)),
            "mesh_vertices_number",
            &corners.concat(),
        );
        for edge in 0..4 {
            coords(
                row(rows, &role(40 + offset + edge, None)),
                "line_numbers",
                &[corners[edge], corners[(edge + 1) % 4]].concat(),
            );
            coords(
                row(rows, &role(44 + offset + edge, None)),
                "point_numbers",
                &corners[edge],
            );
        }
        coords(
            row(rows, &role(48 + offset, None)),
            "named_numbers",
            &[x0, y1, 0., x1 - x0],
        );
        coords(
            row(rows, &role(176 + 5 * plane, None)),
            "property_value_candidate_0",
            &bounds,
        );
    }
    let original = &seed["graph"]["owners"]["DesignViewSegment"][2]["recipe"]["fields"];
    let nums: Vec<f64> = (0..14)
        .map(|i| {
            original
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["name"] == format!("camera_number_{i}"))
                .unwrap()["value"]
                .as_f64()
                .unwrap()
        })
        .collect();
    let scale = ((0..3).map(|i| (b[i + 3] - b[i]).powi(2)).sum::<f64>() / 3.).sqrt();
    let r = &mut g["owners"]["DesignViewSegment"][2];
    for i in 0..3 {
        let center = (b[i] + b[i + 3]) * 0.5;
        set(r, &format!("camera_number_{i}"), json!(center));
        set(
            r,
            &format!("camera_number_{}", i + 3),
            json!(center + (nums[i + 3] - nums[i]) * scale),
        );
    }
    for i in [6, 7, 9, 10] {
        set(r, &format!("camera_number_{i}"), json!(nums[i] * scale));
    }
}
fn ufrx(seed: &Value, doc: &PreparedAssembly, bounds: &[f64; 6]) -> Vec<Value> {
    let source = seed["ufrx_fields"].as_array().unwrap();
    let refs: Vec<_> = source
        .iter()
        .filter(|f| f["name"].as_str().unwrap().starts_with("reference_0_"))
        .collect();
    let placement: Vec<_> = source
        .iter()
        .filter(|f| f["name"].as_str().unwrap().starts_with("occurrence_0_"))
        .collect();
    let mut out = Vec::new();
    let mut uses = vec![0; doc.definitions.len()];
    for old in source {
        let name = old["name"].as_str().unwrap();
        if name.starts_with("reference_0_") {
            if name != refs[0]["name"] {
                continue;
            }
            for (j, def) in doc.definitions.iter().enumerate() {
                for &field in &refs {
                    let mut f = field.clone();
                    let name = field["name"]
                        .as_str()
                        .unwrap()
                        .strip_prefix("reference_0_")
                        .unwrap();
                    f["name"] = json!(format!("reference_{j}_{name}"));
                    if f["policy"] == "part-path" {
                        f["reference_index"] = json!(j);
                    } else {
                        match name {
                            "document_id" => f["value"] = json!(def.part.document_id),
                            "revision_id" => f["value"] = json!(def.part.database_revision_id),
                            "reference_id" => f["value"] = json!(def.reference_id),
                            "occurrence_count" => {
                                f["value"] = json!(doc
                                    .occurrences
                                    .iter()
                                    .filter(|o| o.reference_id == def.reference_id)
                                    .count())
                            }
                            _ => {}
                        }
                    }
                    out.push(f);
                }
            }
        } else if name.starts_with("occurrence_0_") {
            if name != placement[0]["name"] {
                continue;
            }
            for (i, o) in doc.occurrences.iter().enumerate() {
                let j = o.reference_id as usize - 1;
                uses[j] += 1;
                let repeat = uses[j] > 1;
                let part = &doc.definitions[j].part;
                for &field in &placement {
                    let name = field["name"]
                        .as_str()
                        .unwrap()
                        .strip_prefix("occurrence_0_")
                        .unwrap();
                    if repeat
                        && [28, 30, 44, 45]
                            .iter()
                            .any(|t| name.contains(&format!("section_0_tag_{t}_")))
                    {
                        continue;
                    }
                    let mut f = field.clone();
                    f["name"] = json!(format!("occurrence_{i}_{name}"));
                    f["value"] = match name {
                        "occurrence_id" | "header_value" => json!(i + 1),
                        "title_marker" => json!(uses[j]),
                        "reference_id" => json!(o.reference_id),
                        "title" => json!(o.name),
                        "section_0_0" | "section_0_1" => json!(if repeat { 16 } else { 20 }),
                        "section_0_tag_3_property_value" => json!(part.document_id),
                        "section_0_tag_5_property_value" => {
                            json!(part.relative_path[..part.relative_path.len() - 4])
                        }
                        "section_0_tag_30_property_value" | "section_1_tag_30_property_value" => {
                            json!(part.display_name)
                        }
                        "section_0_tag_2_property_value" => json!(uuid::Uuid::new_v5(
                            &uuid::Uuid::parse_str(&part.document_id).unwrap(),
                            b"assembly-reference-key-v1"
                        )
                        .to_string()),
                        _ => f["value"].clone(),
                    };
                    out.push(f);
                }
            }
        } else {
            let mut f = old.clone();
            match name {
                "reference_count" | "representation_reference_count" => {
                    f["value"] = json!(doc.definitions.len())
                }
                "occurrence_count" | "representation_placement_count" => {
                    f["value"] = json!(doc.occurrences.len())
                }
                _ => {
                    if let Some(index) = name.strip_prefix("final_bounds_coordinate_") {
                        f["value"] = json!(bounds[index.parse::<usize>().unwrap()]);
                    }
                }
            };
            out.push(f);
        }
    }
    out
}

pub(super) fn expand(doc: &PreparedAssembly) -> Result<Value> {
    let seed: Value =
        serde_json::from_str(include_str!("profile.json")).map_err(super::super::err)?;
    if seed["schema"] != 1 || seed["profile"] != PROFILE_ID {
        return Err(Error("unsupported IAM defaults".into()));
    }
    let mut g = graph(&seed, doc);
    let n = doc.occurrences.len();
    let d = doc.definitions.len();
    let rx = &mut g["owners"][RX];
    let base = row(rx, "base/8").clone();
    let at = rx
        .as_array()
        .unwrap()
        .iter()
        .position(|r| r["role"] == "base/8")
        .unwrap();
    rx.as_array_mut().unwrap().splice(
        at..at + 1,
        (0..d).map(|j| {
            let mut r = base.clone();
            r["role"] = json!(format!("def/{j}"));
            r
        }),
    );
    for (j, def) in doc.definitions.iter().enumerate() {
        let name = format!("def/{j}");
        let t = doc
            .occurrences
            .iter()
            .enumerate()
            .filter(|(_, o)| o.reference_id == def.reference_id)
            .map(|(i, _)| role(9, Some(i)))
            .collect();
        members(rx, &name, "part_members", t, None);
        let r = row(rx, &name);
        set(r, "part_display_name", json!(def.part.display_name));
        set(r, "part_state_unknown_0", json!(j + 1));
        set(r, "part_bom_id", json!(j + 1));
    }
    members(
        rx,
        "base/6",
        "rx_definitions",
        (0..d).map(|j| format!("def/{j}")).collect(),
        None,
    );
    set(row(rx, "base/6"), "rx_definition_count_candidate", json!(d));
    for (i, o) in doc.occurrences.iter().enumerate() {
        let b = doc.definitions[o.reference_id as usize - 1]
            .part
            .bounds_cm
            .unwrap();
        for (owner, slot) in [(DC, 25), (BR, 12)] {
            set(
                row(&mut g["owners"][owner], &role(slot, Some(i))),
                "display_name",
                json!(o.name),
            );
        }
        for (owner, slot, prefix) in [
            (DC, 27, "bounds_coordinate"),
            (DC, 36, "context_bounds_coordinate"),
            (RX, 5, "rx_bounds_coordinate"),
        ] {
            coords(
                row(&mut g["owners"][owner], &role(slot, Some(i))),
                prefix,
                &b,
            );
        }
    }
    let mut bounds = [f64::INFINITY; 6];
    bounds[3..].fill(f64::NEG_INFINITY);
    for def in &doc.definitions {
        let b = def.part.bounds_cm.unwrap();
        for i in 0..3 {
            bounds[i] = bounds[i].min(b[i]);
            bounds[i + 3] = bounds[i + 3].max(b[i + 3]);
        }
    }
    drawing(&mut g, &bounds, &seed);
    // A display name is an explicit input. Saved view/model-state labels retain
    // the declared profile defaults and are not inferred from filesystem names.
    set(
        row(&mut g["owners"][DC], "base/0"),
        "document_name",
        json!(doc.document_name),
    );
    let table = tables(&seed, &g, n);
    let uf = ufrx(&seed, doc, &bounds);
    if uf.len() != 343 + 220 * n + 38 * d {
        return Err(Error("IAM reference field census differs".into()));
    }
    Ok(json!({"graph":g,"tables":table,"ufrx_fields":uf}))
}
