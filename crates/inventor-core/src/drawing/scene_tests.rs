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
            ("font_size_parameters", FieldValue::F32(vec![0.5, 1.])),
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

fn attach_attribute(doc: &mut DrawingInventory, ordinal: usize, attribute: PayloadObservation) {
    let o = &mut doc.segments[1].observations[ordinal];
    o.fields.push(FieldObservation {
        name: "attribute_reference",
        value: w(101),
        source: source(),
    });
    doc.segments[1].observations.push(observation(
        100,
        "display_attributes_candidate",
        vec![("attribute_entry", w(102))],
    ));
    doc.segments[1].observations.push(attribute);
}

#[test]
fn visibility_is_attribute_driven_inherited_and_rejects_broken_links() {
    for owner in [0, 1] {
        let mut d = placement_doc();
        attach_attribute(
            &mut d,
            owner,
            observation(
                101,
                "display_boolean_candidate",
                vec![
                    ("attribute_mask", w(4)),
                    ("attribute_boolean", FieldValue::U8(0)),
                ],
            ),
        );
        let result = experimental_scene(&d, &Limits::default());
        assert_eq!(result.spaces.len(), 1);
        assert!(result.spaces[0].items.is_empty());
        assert!(result.spaces[0]
            .omitted
            .iter()
            .any(|x| x.record_ordinal == 1 && x.reason == "hidden_by_stored_attribute"));
        d.segments[1].observations.last_mut().unwrap().fields[1].value = FieldValue::U8(1);
        assert_eq!(
            experimental_scene(&d, &Limits::default()).spaces[0]
                .items
                .len(),
            1
        );
        d.segments[1].observations.last_mut().unwrap().fields[1].value = FieldValue::U8(9);
        assert!(experimental_scene(&d, &Limits::default()).spaces.is_empty());
        d.segments[1].observations.pop();
        assert!(experimental_scene(&d, &Limits::default()).spaces.is_empty());
    }
}

#[test]
fn local_layer_cache_binding_retains_revision_uncertainty_and_applies_width() {
    let mut d = placement_doc();
    attach_attribute(
        &mut d,
        0,
        observation(
            101,
            "layer_binding_candidate",
            vec![
                ("layer_binding_mask", w(1)),
                ("layer_reference", w(1)),
                ("color_override", w(u32::MAX)),
                ("stroke_scale", FieldValue::F32(vec![2.])),
            ],
        ),
    );
    let mut meta = placement_doc().segments.remove(0).meta.unwrap();
    meta.reference_tables[0].bytes[..16].copy_from_slice(&[8; 16]);
    meta.reference_tables[1].bytes[..16].copy_from_slice(&[9; 16]);
    d.segments[1].meta = Some(meta);
    d.segments.push(segment(
        "AppSegmentType",
        [8; 16],
        vec![observation(
            7,
            "layer_definition_candidate",
            vec![
                ("header_flags", w(0)),
                ("object_id", FieldValue::U16(77)),
                (
                    "layer_name",
                    FieldValue::Utf16("arbitrary renamed layer".into()),
                ),
                ("layer_width", FieldValue::F64(vec![0.03])),
                ("layer_rgba", FieldValue::F32(vec![0.2, 0.3, 0.4, 1.])),
                ("line_pattern", w(0x6dc4)),
            ],
        )],
    ));
    let result = experimental_scene(&d, &Limits::default());
    let style = &result.spaces[0].items[0].style;
    assert_eq!(style.width, Some(0.06));
    assert_eq!(style.dash, Some(vec![]));
    assert_eq!(style.layer.as_deref(), Some("arbitrary renamed layer"));
    assert!(style
        .unresolved
        .contains(&"layer_revision_binding_unverified"));
    // Duplicated object keys cannot be resolved by first-match or by label.
    d.segments[2].observations.push(observation(
        8,
        "layer_definition_candidate",
        vec![("header_flags", w(0)), ("object_id", FieldValue::U16(77))],
    ));
    assert!(experimental_scene(&d, &Limits::default()).spaces.is_empty());
}

#[test]
fn dash_overrides_and_rotated_text_basis_preserve_stored_values() {
    let mut d = placement_doc();
    attach_attribute(
        &mut d,
        1,
        observation(
            101,
            "stroke_override_candidate",
            vec![
                ("stroke_mask", w(8)),
                ("stroke_width", FieldValue::F32(vec![0.025])),
                (
                    "stroke_dashes",
                    FieldValue::F64(vec![0.4, -0.1, 0.02, -0.1]),
                ),
            ],
        ),
    );
    let scene = experimental_scene(&d, &Limits::default());
    assert_eq!(
        scene.spaces[0].items[0].style.dash,
        Some(vec![0.4, 0.1, 0.02, 0.1])
    );
    d.segments[1].observations.last_mut().unwrap().fields[2].value =
        FieldValue::F64(vec![0.4, 0.1]);
    assert!(experimental_scene(&d, &Limits::default()).spaces.is_empty());
    let t = observation(
        0,
        "stored_text_candidate",
        vec![
            ("text", FieldValue::Utf16("寸法\n<>".into())),
            (
                "position_and_direction_candidate",
                FieldValue::F64(vec![3., 4., 2., 0., 1., 0.]),
            ),
            ("style_index_candidate", w(99)),
            ("raw_text_flags", FieldValue::U16(9)),
        ],
    );
    let mut m = IDENTITY;
    m[0][0] = -1.;
    m[0][3] = 10.;
    let Some(DisplayGeometry::Text {
        text,
        position,
        direction,
        up,
        font,
        ..
    }) = geometry(&t, &m, &BTreeMap::new(), &mut 100).unwrap()
    else {
        panic!()
    };
    assert_eq!(text, "寸法\n<>");
    assert_eq!(position, [7., 4., 2.]);
    assert_eq!(direction, [0., 1., 0.]);
    assert_eq!(up, [1., 0., 0.]);
    assert!(font.is_none());
}

fn sheet_document() -> DrawingInventory {
    let mut d = placement_doc();
    let context = [4; 16];
    let doc_id = [9; 16];
    d.segments[0].observations[0].fields.push(FieldObservation {
        name: "definition_reference",
        value: w(2),
        source: source(),
    });
    let meta = d.segments[0].meta.as_mut().unwrap();
    meta.reference_tables[0] = table(7, [[3; 16], context, doc_id, context].concat(), 32);
    meta.reference_tables[1] = table(
        8,
        [
            context.to_vec(),
            vec![0; 4],
            context.to_vec(),
            vec![1, 0, 0, 0],
        ]
        .concat(),
        20,
    );
    meta.reference_tables[2] = table(
        10,
        vec![0, 0, 0, 0, 0, 0, 77, 0, 1, 0, 18, 0, 0, 0, 42, 0],
        8,
    );
    let definition = |id, key| {
        observation(
            id,
            "sheet_segment_links_candidate",
            vec![
                ("header_flags", w(18)),
                ("object_id", FieldValue::U16(key)),
                ("name", FieldValue::Utf16("同名のシート".into())),
            ],
        )
    };
    d.segments.push(segment(
        "DlDocDcSegmentType",
        doc_id,
        vec![
            observation(
                3,
                "document_sheet_list_candidate",
                vec![(
                    "sheet_references_candidate",
                    FieldValue::U32(vec![0x80000016, 0x8000000d]),
                )],
            ),
            definition(12, 42),
            definition(21, 43),
            definition(30, 44),
        ],
    ));
    let mut other = placement_doc().segments.remove(0);
    other.registry.id = guid(&[8; 16]);
    other.meta = Some(MetaInventory {
        source: source(),
        compressed_source: source(),
        codec: "synthetic",
        expanded_bytes: 0,
        state_words: [0; 3],
        block_words: vec![],
        block_table_source: source(),
        types: vec![],
        reference_tables: vec![
            table(7, [doc_id, context].concat(), 32),
            table(8, [context.to_vec(), vec![0; 4]].concat(), 20),
            table(10, vec![0, 0, 18, 0, 0, 0, 43, 0], 8),
        ],
    });
    other.observations[0].fields.push(FieldObservation {
        name: "definition_reference",
        value: w(1),
        source: source(),
    });
    d.segments.push(other);
    d
}

#[test]
fn sheets_follow_list_order_and_full_backlinks_without_names_or_units_guesses() {
    let mut d = sheet_document();
    d.segments.reverse();
    let sheets = stored_sheets(&d, &Limits::default());
    assert_eq!(sheets.status, "stored_order_unverified_state");
    assert_eq!(sheets.sheets.len(), 2);
    assert_eq!(sheets.sheets[0].definition_record, 21);
    assert_eq!(sheets.sheets[1].definition_record, 12);
    assert_eq!(sheets.sheets[0].name, sheets.sheets[1].name);
    assert_ne!(sheets.sheets[0].id, sheets.sheets[1].id);
    assert_eq!(sheets.sheets[0].space_segment, Some(guid(&[8; 16])));
    assert_eq!(sheets.sheets[1].space_segment, Some(guid(&[2; 16])));
    assert_eq!(sheets.sheets[0].size_in_source_units, Some([42., 29.7]));
    assert_eq!(sheets.length_unit, None);
    assert_eq!(sheets.millimeters_per_unit, None);
    assert!(!sheets.qualified);
}

#[test]
fn sheets_reject_corrupt_lists_and_keep_ambiguous_backlinks_unavailable() {
    for raw in [0, 0x80000000, 0x800000ff, 12] {
        let mut d = sheet_document();
        d.segments[2].observations[0].fields[0].value = FieldValue::U32(vec![raw]);
        assert_eq!(stored_sheets(&d, &Limits::default()).status, "unavailable");
    }
    let mut d = sheet_document();
    d.segments[2].observations[0].fields[0].value = FieldValue::U32(vec![0x8000000d; 2]);
    assert!(stored_sheets(&d, &Limits::default()).sheets.is_empty());
    let mut d = sheet_document();
    d.segments[0].meta.as_mut().unwrap().reference_tables[1].bytes[20] ^= 1;
    let sheets = stored_sheets(&d, &Limits::default());
    assert_eq!(sheets.sheets[1].status, "unavailable");
    assert!(sheets.sheets[1].space_segment.is_none());
    let d = sheet_document();
    let small = Limits {
        max_records: 10,
        ..Limits::default()
    };
    assert!(stored_sheets(&d, &small).sheets.is_empty());
}
