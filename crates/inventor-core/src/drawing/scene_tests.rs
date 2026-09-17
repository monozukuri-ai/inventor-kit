use super::*;
use std::io::Cursor;

fn source() -> SourceSpan {
    SourceSpan::stream("synthetic", "/B", 0, 0)
}
fn observation(
    id: usize,
    role: &'static str,
    values: Vec<(&'static str, FieldValue)>,
) -> PayloadObservation {
    PayloadObservation {
        record_ordinal: id,
        type_id: "synthetic".into(),
        source: source(),
        layout: "synthetic",
        proposed_role: role,
        status: "unqualified",
        fields: values
            .into_iter()
            .map(|(name, value)| FieldObservation {
                name,
                value,
                source: source(),
            })
            .collect(),
    }
}
fn w(n: u32) -> FieldValue {
    FieldValue::U32(vec![n])
}
fn group(id: usize, owner: u32, children: &[u32]) -> PayloadObservation {
    observation(
        id,
        "display_group_candidate",
        vec![
            ("header_flags", w(0)),
            ("object_id", FieldValue::U16(77)),
            ("owner_reference", w(owner)),
            (
                "child_references_unresolved",
                FieldValue::U32(children.into()),
            ),
            ("transform_branch", FieldValue::U8(0)),
        ],
    )
}
fn line(id: usize, owner: u32) -> PayloadObservation {
    observation(
        id,
        "stored_line_candidate",
        vec![
            ("owner_reference", w(owner)),
            (
                "line_endpoints",
                FieldValue::F64(vec![1., 2., 3., 4., 5., 6.]),
            ),
        ],
    )
}
fn segment(kind: &str, id: [u8; 16], observations: Vec<PayloadObservation>) -> SegmentInventory {
    SegmentInventory {
        registry_index: 0,
        registry: SegmentInfo {
            name: "untrusted arbitrary name".into(),
            kind: kind.into(),
            id: guid(&id),
            major: 31,
            source: source(),
        },
        status: "framed",
        profile: None,
        meta: None,
        bulk: None,
        records: vec![],
        observations,
        opaque_regions: vec![],
        diagnostics: vec![],
    }
}
fn document(segments: Vec<SegmentInventory>) -> DrawingInventory {
    let mut c = cfb::CompoundFile::create(Cursor::new(vec![])).unwrap();
    c.set_storage_clsid("/", "bbf9fdf1-52dc-11d0-8c04-0800090be8ec".parse().unwrap())
        .unwrap();
    let mut doc = inspect(
        &c.into_inner().into_inner(),
        "synthetic",
        &Limits::default(),
    )
    .unwrap();
    doc.segments = segments;
    doc
}
fn table(section: u8, bytes: Vec<u8>, size: usize) -> MetaReferenceTable {
    MetaReferenceTable {
        section,
        count: bytes.len() / size,
        source: SourceSpan::stream("synthetic", "/M", 0, bytes.len()),
        bytes,
    }
}
fn placement_doc() -> DrawingInventory {
    let sm_id = [2; 16];
    let dl_id = [3; 16];
    let context = [4; 16];
    let mut placement = observation(
        0,
        "sheet_space_candidate",
        vec![
            ("display_reference", w(1)),
            ("references_unresolved", FieldValue::U32(vec![])),
            (
                "origin_and_extent_candidate",
                FieldValue::F64(vec![0., 0., 42., 29.7]),
            ),
        ],
    );
    add_matrix(&mut placement, IDENTITY);
    let mut sm = segment("DlSheetSmSegmentType", sm_id, vec![placement]);
    sm.meta = Some(MetaInventory {
        source: source(),
        compressed_source: source(),
        codec: "synthetic",
        expanded_bytes: 0,
        state_words: [0; 3],
        block_words: vec![],
        block_table_source: source(),
        types: vec![],
        reference_tables: vec![
            table(7, [dl_id, context].concat(), 32),
            table(8, [context.to_vec(), vec![0; 4]].concat(), 20),
            table(10, vec![0, 0, 0, 0, 0, 0, 77, 0], 8),
        ],
    });
    // Object id 77 deliberately differs from ordinal 0; registry names are not used.
    document(vec![
        sm,
        segment(
            "DlSheetDlSegmentType",
            dl_id,
            vec![group(0, 0, &[0x80000002]), line(1, 0x80000001)],
        ),
    ])
}
fn add_matrix(o: &mut PayloadObservation, m: Matrix) {
    let mut values = [0.; 16];
    for i in 0..4 {
        values[4 * i..4 * i + 4].copy_from_slice(&m[i]);
    }
    o.fields.push(FieldObservation {
        name: "transform_candidate",
        value: FieldValue::CompactTransform {
            prefixed: false,
            set: 0,
            zero: 0,
            values,
        },
        source: source(),
    });
}

#[test]
fn binding_uses_context_segment_guid_and_object_key_and_keeps_evidence() {
    let d = placement_doc();
    let scene = experimental_scene(&d, &Limits::default());
    assert_eq!(scene.status, "experimental_partial");
    assert!(!scene.qualified);
    assert!(scene.diagnostics.is_empty());
    assert_eq!(scene.spaces[0].items.len(), 1);
    assert_eq!(scene.spaces[0].bindings[0].target_record, 0);
    assert_eq!(scene.spaces[0].bindings[0].sources.len(), 5);
    // Every mutation must discard the affected space, not return an earlier partial branch.
    for mutation in 0..6 {
        let mut d = placement_doc();
        match mutation {
            0 => d.segments[0].meta.as_mut().unwrap().reference_tables[1].bytes[0] ^= 1,
            1 => d.segments[0].meta.as_mut().unwrap().reference_tables[2].bytes[0] = 9,
            2 => d.segments[0].meta.as_mut().unwrap().reference_tables[2].bytes[6] = 0,
            3 => d.segments[1].observations.push(group(7, 0, &[])),
            4 => d.segments[1].registry.id = guid(&[9; 16]),
            5 => d.segments[1].observations[1].fields[0].value = w(0),
            _ => unreachable!(),
        }
        let scene = experimental_scene(&d, &Limits::default());
        assert_eq!(scene.status, "unavailable", "mutation {mutation}");
        assert!(scene.spaces.is_empty());
        assert!(!scene.diagnostics.is_empty());
    }
    let d = placement_doc();
    let limits = Limits {
        max_records: 1,
        ..Limits::default()
    };
    let scene = experimental_scene(&d, &limits);
    assert!(scene.spaces.is_empty());
}

#[test]
fn owners_reject_dangling_duplicate_missing_backlinks_and_cycles() {
    for observations in [
        vec![group(0, 0, &[0x80000002])],
        vec![group(0, 0, &[0x80000002, 0x80000002]), line(1, 0x80000001)],
        vec![group(0, 0, &[]), line(1, 0x80000001)],
        vec![
            group(0, 0x80000002, &[0x80000002]),
            group(1, 0x80000001, &[0x80000001]),
        ],
    ] {
        assert!(owners(
            &segment("DlSheetDlSegmentType", [3; 16], observations),
            &mut 1000
        )
        .is_err());
    }
    let s = segment(
        "DlSheetDlSegmentType",
        [3; 16],
        vec![group(0, 0, &[0, 0x80000002]), line(1, 0x80000001)],
    );
    assert!(owners(&s, &mut 100).is_ok());
    assert!(owners(&s, &mut 1).is_err());
}

#[test]
fn parent_rotation_precedes_local_translation_and_preserves_depth() {
    let mut parent = IDENTITY;
    parent[0] = [0., -1., 0., 10.];
    parent[1] = [1., 0., 0., 20.];
    let mut local = IDENTITY;
    local[0][3] = 3.;
    local[1][3] = 4.;
    let m = compose(&parent, &local).unwrap();
    let DisplayGeometry::Polyline { points } =
        geometry(&line(1, 0), &m, &BTreeMap::new(), &mut 100)
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(points, vec![[4., 24., 3.], [1., 27., 6.]]);
    // Exercise the same order through the actual sheet and nested display traversal.
    let mut d = placement_doc();
    d.segments[0].observations[0].fields.pop();
    add_matrix(&mut d.segments[0].observations[0], parent);
    let root = &mut d.segments[1].observations[0];
    root.fields.last_mut().unwrap().value = FieldValue::U8(1);
    add_matrix(root, local);
    let scene = experimental_scene(&d, &Limits::default());
    let DisplayGeometry::Polyline { points: actual } = &scene.spaces[0].items[0].geometry else {
        panic!()
    };
    assert_eq!(actual, &points);
    assert_eq!(scene.spaces[0].items[0].group_path, vec![0]);
    // A projective matrix must not be mistaken for an affine drawing transform.
    let mut bad = IDENTITY;
    bad[3][0] = 1.;
    let mut o = group(0, 0, &[]);
    add_matrix(&mut o, bad);
    assert!(matrix(&o).is_err());
}

#[test]
fn analytic_arc_transforms_vectors_without_translation_and_rejects_invalid_basis() {
    let mut values = vec![
        2.,
        3.,
        4.,
        0.,
        0.,
        1.,
        1.,
        0.,
        0.,
        2.,
        0.,
        std::f64::consts::FRAC_PI_2,
    ];
    let mut m = IDENTITY;
    m[0] = [0., -1., 0., 10.];
    m[1] = [1., 0., 0., 20.];
    let arc = |v| {
        observation(
            1,
            "stored_arc_candidate",
            vec![("arc_center_normal_axis_radius_angles", FieldValue::F64(v))],
        )
    };
    let DisplayGeometry::Curve { center, u, v, .. } =
        geometry(&arc(values.clone()), &m, &BTreeMap::new(), &mut 100)
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(center, [7., 22., 4.]);
    assert_eq!(u, [0., 2., 0.]);
    assert_eq!(v, [-2., 0., 0.]);
    values[5] = 2.;
    assert!(geometry(&arc(values.clone()), &m, &BTreeMap::new(), &mut 100).is_err());
    values[5] = 1.;
    values[9] = -2.;
    assert!(geometry(&arc(values), &m, &BTreeMap::new(), &mut 100).is_err());
}

#[test]
fn exact_line_circle_arc_fields_reject_truncation_extra_bytes_and_nonfinite() {
    for (typ, values, suffix) in [
        (
            "a79eacc7-11d1-c281-6000-a38ab46bceb0",
            vec![1., 2., 3., 4., 5., 6.],
            false,
        ),
        (
            "a79eaccd-11d1-c281-6000-a38ab46bceb0",
            vec![1., 2., 3., 0., 0., 1., 2.],
            true,
        ),
        (
            "a79eaccc-11d1-c281-6000-a38ab46bceb0",
            vec![1., 2., 3., 0., 0., 1., 1., 0., 0., 2., 0., 1.],
            true,
        ),
    ] {
        let mut b = vec![0; 26];
        for v in values {
            b.extend(f64::to_le_bytes(v));
        }
        if suffix {
            b.push(0);
        }
        let decode = |data: &[u8]| {
            super::super::fields::decode(
                "DlSheetDlSegmentType",
                typ,
                0,
                data,
                SourceSpan::stream("synthetic", "/B", 0, data.len()),
                &mut 1000,
            )
        };
        assert!(decode(&b).unwrap().is_some());
        for end in 0..b.len() {
            assert!(decode(&b[..end]).is_err());
        }
        let mut extra = b.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
        b[26..34].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&b).is_err());
    }
}

#[test]
fn fonts_use_stored_identifier_and_reject_duplicates() {
    let entry = || {
        vec![
            ("font_id", w(77)),
            ("font_tag", FieldValue::U16(4)),
            ("font_weight", FieldValue::U16(700)),
            ("font_flags", FieldValue::U16(0)),
            ("font_size_parameters", FieldValue::F32(vec![0.5, 0.])),
            ("font_name", FieldValue::Utf16("Synthetic font".into())),
            ("font_tail_parameters", FieldValue::F32(vec![0., 1., 0.])),
        ]
    };
    let mut row = entry();
    row.push(("font_next_id", w(78)));
    let d = document(vec![segment(
        "DlDirectorySegmentType",
        [4; 16],
        vec![observation(0, "font_table_candidate", row)],
    )]);
    let f = fonts(&d, &mut 100).unwrap();
    assert!(!f.contains_key(&1));
    assert_eq!(f[&77].family, "Synthetic font");
    let mut rows = entry();
    rows.extend(entry());
    rows.push(("font_next_id", w(78)));
    let d = document(vec![segment(
        "DlDirectorySegmentType",
        [4; 16],
        vec![observation(0, "font_table_candidate", rows)],
    )]);
    assert!(fonts(&d, &mut 100).is_err());
}
