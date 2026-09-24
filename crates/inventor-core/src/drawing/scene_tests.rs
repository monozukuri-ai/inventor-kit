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

fn revised_placement_doc() -> DrawingInventory {
    let mut d = placement_doc();
    let mut bytes = [3u32.to_le_bytes(), 2u32.to_le_bytes()].concat();
    for id in [4u8, 5] {
        bytes.extend([id; 16]);
        bytes.extend([0; 6]);
    }
    d.revisions = Some(
        super::super::revisions::decode(
            &bytes,
            SourceSpan::stream("synthetic", "/R", 0, bytes.len()),
            &mut 100,
        )
        .unwrap(),
    );
    let tables = &mut d.segments[0].meta.as_mut().unwrap().reference_tables;
    tables[0].bytes[16..32].fill(5); // Current revision; object namespace remains 4.
    tables[2].bytes[2..6].copy_from_slice(&0x12u32.to_le_bytes());
    d.segments[1].observations[0].fields[0].value = w(0x12);
    let key = (0x12u64 << 16) | 77;
    let ranges: Vec<_> = [(0u32, key), (1, key + 1)]
        .into_iter()
        .flat_map(|(r, k)| [r.to_le_bytes().as_slice(), &k.to_le_bytes()[..6]].concat())
        .collect();
    let mut meta = placement_doc().segments.remove(0).meta.unwrap();
    meta.reference_tables = vec![table(2, ranges, 10)];
    d.segments[1].meta = Some(meta);
    d
}

#[test]
fn saved_revision_binding_requires_both_exact_id_interval_and_current_revision() {
    let d = revised_placement_doc();
    let (target, object, spans, verified) =
        resolve(&d, &d.segments[0], 1, "DlSheetDlSegmentType", &mut 100).unwrap();
    assert!(verified);
    assert_eq!(target.registry.kind, "DlSheetDlSegmentType");
    assert_eq!(object.record_ordinal, 0);
    assert_eq!(spans.len(), 8);
    assert_eq!(spans[3].end_offset - spans[3].start_offset, 10);
    assert_eq!(
        experimental_scene(&d, &Limits::default()).spaces[0]
            .items
            .len(),
        1
    );
    // SampleBg repeats a revision index (sometimes with a larger bound). This
    // retains one namespace identity and is not an ambiguous object key.
    let mut repeated = revised_placement_doc();
    let range = &mut repeated.segments[1].meta.as_mut().unwrap().reference_tables[0];
    let prefix = range.bytes[..10].to_vec();
    range.bytes.splice(0..0, prefix);
    range.count += 1;
    range.source.end_offset += 10;
    assert!(resolve(
        &repeated,
        &repeated.segments[0],
        1,
        "DlSheetDlSegmentType",
        &mut 100
    )
    .is_ok());
    for mutation in 0..10 {
        let mut d = revised_placement_doc();
        match mutation {
            0 => d.revisions = None,
            1 => d.segments[1].meta = None,
            2 => d.segments[0].meta.as_mut().unwrap().reference_tables[0].bytes[16] ^= 1,
            3 => d.segments[0].meta.as_mut().unwrap().reference_tables[1].bytes[0] ^= 1,
            4 => d.segments[1].meta.as_mut().unwrap().reference_tables[0].bytes[4] -= 1, // Inclusive boundary: key now belongs to another namespace.
            5 => d.segments[1].meta.as_mut().unwrap().reference_tables[0].bytes[10] = 0, // Wrong current revision.
            6 => d.segments[1].meta.as_mut().unwrap().reference_tables[0].bytes[14] = 0, // Decreasing bound.
            7 => d.segments[1].meta.as_mut().unwrap().reference_tables[0].bytes[10] = 2, // Missing revision.
            8 => d.segments[1].observations[0].fields[0].value = w(0), // Same short ID, different upper key.
            9 => d.segments[1].meta.as_mut().unwrap().reference_tables[0].count = 3,
            _ => unreachable!(),
        }
        let scene = experimental_scene(&d, &Limits::default());
        assert!(scene.spaces.is_empty(), "mutation {mutation}");
        assert!(!scene.diagnostics.is_empty());
    }
    assert!(resolve(&d, &d.segments[0], 1, "DlSheetDlSegmentType", &mut 2).is_err());
}

#[test]
fn major23_historical_context_requires_the_targets_exact_revision_range() {
    let mut d = revised_placement_doc();
    for s in &mut d.segments {
        s.registry.major = 23;
    }
    // The target retains a later revision, while the reference names its older
    // context, which is still in the exact object-identity range table.
    d.segments[0].meta.as_mut().unwrap().reference_tables[0].bytes[16..32].fill(4);
    let (_, _, sources, verified) =
        resolve(&d, &d.segments[0], 1, "DlSheetDlSegmentType", &mut 100).unwrap();
    assert!(verified);
    assert_eq!(sources.len(), 10); // Includes both historical and latest evidence.
    let mut cache = ResolveCache::default();
    resolve_cached(
        &d,
        &d.segments[0],
        1,
        "DlSheetDlSegmentType",
        &mut 100,
        &mut cache,
    )
    .unwrap();
    assert!(resolve_cached(
        &d,
        &d.segments[0],
        1,
        "DlSheetDlSegmentType",
        &mut 0,
        &mut cache
    )
    .is_err());
    assert!(resolve_cached(
        &d,
        &d.segments[0],
        1,
        "AppSegmentType",
        &mut 100,
        &mut cache
    )
    .is_err());
    for s in &mut d.segments {
        s.registry.major = 31;
    }
    assert!(resolve(&d, &d.segments[0], 1, "DlSheetDlSegmentType", &mut 100).is_err());
    for s in &mut d.segments {
        s.registry.major = 23;
    }
    d.segments[0].meta.as_mut().unwrap().reference_tables[0].bytes[16..32].fill(9);
    assert!(resolve(&d, &d.segments[0], 1, "DlSheetDlSegmentType", &mut 100).is_err());
}

#[test]
fn major23_external_display_child_is_omitted_without_local_slot_aliasing() {
    let mut d = placement_doc();
    for s in &mut d.segments {
        s.registry.major = 23;
    }
    let children = d.segments[1].observations[0]
        .fields
        .iter_mut()
        .find(|f| f.name == "child_references_unresolved")
        .unwrap();
    let FieldValue::U32(ref mut refs) = children.value else {
        panic!()
    };
    refs.push(1);
    let scene = experimental_scene(&d, &Limits::default());
    assert_eq!(scene.spaces.len(), 1);
    assert!(scene.spaces[0].items.is_empty());
    assert!(scene.spaces[0]
        .omitted
        .iter()
        .any(|o| o.reason == "cross_segment_display_children_not_supported"));
    for s in &mut d.segments {
        s.registry.major = 31;
    }
    assert!(experimental_scene(&d, &Limits::default()).spaces.is_empty());
}

#[test]
fn local_sm_display_uses_tagged_slots_and_validates_ownership() {
    let make = || {
        let mut d = placement_doc();
        d.segments[0].observations[0].fields[1].value = FieldValue::U32(vec![0x80000002]);
        d.segments[0].observations.extend([
            observation(
                1,
                "sheet_local_display_candidate",
                vec![
                    ("display_reference", w(0x80000003)),
                    ("references_unresolved", FieldValue::U32(vec![])),
                ],
            ),
            group(2, 0, &[0x80000004]),
            line(3, 0x80000003),
        ]);
        d
    };
    let d = make();
    let scene = experimental_scene(&d, &Limits::default());
    assert_eq!(scene.spaces[0].items.len(), 2);
    assert_eq!(
        scene.spaces[0].bindings[1].target_segment,
        d.segments[0].registry.id
    );
    for mutation in 0..6 {
        let mut d = make();
        let nodes = &mut d.segments[0].observations;
        match mutation {
            0 => nodes[1].fields[0].value = w(3), // Untagged cross-segment reference.
            1 => nodes[1].fields[0].value = w(0x80000008),
            2 => nodes.push(group(2, 0, &[])),
            3 => nodes[2].fields[2].value = w(0x80000004),
            4 => nodes[3].fields[0].value = w(0),
            5 => nodes[0].fields[1].value = FieldValue::U32(vec![0x80000002, 0x80000002]),
            _ => unreachable!(),
        }
        assert!(
            experimental_scene(&d, &Limits::default()).spaces.is_empty(),
            "mutation {mutation}"
        );
    }
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
    let limits = DrawingLimits {
        max_reference_visits: 1,
        ..DrawingLimits::default()
    };
    let scene = experimental_scene_with_limits(&d, &Limits::default(), &limits);
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
                31.into(),
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
fn filled_annotation_conics_require_observed_sm_profiles_and_keep_chord_intervals() {
    for (typ, values) in [
        (
            "a79eaccd-11d1-c281-6000-a38ab46bceb0",
            vec![1., 2., 0., 0., 0., 1., 0.0625],
        ),
        (
            "a79eaccc-11d1-c281-6000-a38ab46bceb0",
            vec![
                1.,
                2.,
                0.,
                0.,
                0.,
                1.,
                1.,
                0.,
                0.,
                0.25,
                0.,
                std::f64::consts::FRAC_PI_2,
            ],
        ),
    ] {
        let mut b = vec![0; 26];
        for v in &values {
            b.extend(v.to_le_bytes());
        }
        b.push(1);
        let decode = |kind: &str, major: u8, data: &[u8], work: &mut usize| {
            super::super::fields::decode(kind, major.into(), typ, 0, data, source(), work)
        };
        for major in [23, 24, 26, 28, 29, 31] {
            for kind in ["DlSheetDlSegmentType", "DlSheetSmSegmentType"] {
                let result = decode(kind, major, &b, &mut 1000);
                if !(major == 26 || (major == 28 && values.len() == 7))
                    || kind != "DlSheetSmSegmentType"
                {
                    assert!(result.is_err());
                    continue;
                }
                let o = result.unwrap().unwrap();
                let mut m = IDENTITY;
                m[0][3] = 10.;
                let DisplayGeometry::Curve {
                    center,
                    filled,
                    start,
                    end,
                    ..
                } = geometry(&o, &m, &BTreeMap::new(), &mut 1000)
                    .unwrap()
                    .unwrap()
                else {
                    panic!()
                };
                assert_eq!(center, [11., 2., 0.]);
                assert_eq!(filled, Some(true));
                assert_eq!(start, 0.);
                assert_eq!(
                    end,
                    if values.len() == 12 {
                        values[11]
                    } else {
                        std::f64::consts::TAU
                    }
                );
                for end in 0..b.len() {
                    assert!(decode(kind, major, &b[..end], &mut 1000).is_err());
                }
                let mut bad = b.clone();
                bad.push(0);
                assert!(decode(kind, major, &bad, &mut 1000).is_err());
                bad = b.clone();
                *bad.last_mut().unwrap() = 2;
                assert!(decode(kind, major, &bad, &mut 1000).is_err());
                assert!(decode(kind, major, &b, &mut 0).is_err());
            }
        }
    }
}

#[test]
fn observed_ellipse_wire_is_exact_and_rejects_unknown_profiles() {
    let typ = "afd5ceeb-11d1-e071-0008-87a406e5dc09";
    let mut data = vec![0; 26];
    for value in [1f64, 2., 0., 3., 2., 1., 0., 0., 0., 1., 0., 0., 1.] {
        data.extend(value.to_le_bytes());
    }
    data.push(0);
    for major in [23, 24, 26, 28, 29, 31] {
        let decode = |bytes: &[u8], work: &mut usize| {
            super::super::fields::decode(
                "DlSheetDlSegmentType",
                major.into(),
                typ,
                0,
                bytes,
                source(),
                work,
            )
        };
        if !matches!(major, 23 | 26 | 28) {
            assert!(decode(&data, &mut 1000).unwrap().is_none());
            continue;
        }
        assert_eq!(
            decode(&data, &mut 1000).unwrap().unwrap().proposed_role,
            "stored_ellipse_candidate"
        );
        for end in 0..data.len() {
            assert!(decode(&data[..end], &mut 1000).is_err());
        }
        let mut bad = data.clone();
        bad.push(0);
        assert!(decode(&bad, &mut 1000).is_err());
        bad = data.clone();
        *bad.last_mut().unwrap() = 1;
        assert!(decode(&bad, &mut 1000).is_err());
        bad = data.clone();
        bad[26..34].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&bad, &mut 1000).is_err());
        assert!(decode(&data, &mut 0).is_err());
    }
    assert!(super::super::fields::decode(
        "DlSheetDlSegmentType",
        26.into(),
        "9b3499d1-11d1-8626-6000-27bd351c3cb0",
        0,
        &[],
        source(),
        &mut 1000
    )
    .unwrap()
    .is_none());
}

#[test]
fn ellipse_basis_radii_and_placement_remain_independent() {
    let values = vec![
        2.,
        3.,
        4.,
        4.,
        2.,
        1.,
        0.,
        0.,
        0.,
        1.,
        0.,
        0.,
        std::f64::consts::FRAC_PI_2,
    ];
    let mut bytes = vec![0; 26];
    for v in &values {
        bytes.extend(v.to_le_bytes());
    }
    bytes.push(0);
    let parse = |b: &[u8], major: u8| {
        super::super::fields::decode(
            "DlSheetDlSegmentType",
            major.into(),
            "afd5ceeb-11d1-e071-0008-87a406e5dc09",
            0,
            b,
            source(),
            &mut 1000,
        )
    };
    let o = parse(&bytes, 23).unwrap().unwrap();
    let mut m = IDENTITY;
    m[0] = [0., -2., 0., 10.];
    m[1] = [3., 0., 0., 20.];
    let DisplayGeometry::Curve {
        center,
        u,
        v,
        start,
        end,
        ..
    } = geometry(&o, &m, &BTreeMap::new(), &mut 100)
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(center, [4., 26., 4.]);
    assert_eq!(u, [0., 12., 0.]);
    assert_eq!(v, [-4., 0., 0.]);
    assert_eq!([start, end], [0., std::f64::consts::FRAC_PI_2]);
    assert!(parse(&bytes, 31).unwrap().is_none());
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end], 23).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(parse(&extra, 23).is_err());
    for (index, value) in [(3, -1.), (4, 0.), (5, 2.), (8, 2.), (12, 0.)] {
        let mut v = values.clone();
        v[index] = value;
        let o = observation(
            1,
            "stored_ellipse_candidate",
            vec![("ellipse_center_radii_axes_angles", FieldValue::F64(v))],
        );
        assert!(geometry(&o, &m, &BTreeMap::new(), &mut 100).is_err());
    }
}

#[test]
fn sampled_spline_uses_shared_point_budget_and_affine_placement() {
    let o = observation(
        1,
        "stored_bspline_candidate",
        vec![
            ("spline_degree", w(2)),
            (
                "spline_knots",
                FieldValue::F64(vec![0., 0., 0., 1., 1., 1.]),
            ),
            ("spline_weights", FieldValue::F64(vec![])),
            (
                "spline_control_points",
                FieldValue::F64(vec![0., 0., 0., 1., 2., 0., 2., 0., 0.]),
            ),
            ("spline_parameter_range", FieldValue::F64(vec![0.25, 0.75])),
        ],
    );
    let mut m = IDENTITY;
    m[0] = [0., -2., 0., 10.];
    m[1] = [2., 0., 0., 20.];
    let drawing = DrawingLimits {
        max_polyline_points: 17,
        ..DrawingLimits::default()
    };
    let mut budget = DisplayBudget::new(&drawing);
    let DisplayGeometry::Polyline { points } =
        geometry_budgeted(&o, &m, &BTreeMap::new(), &mut 100, &mut budget)
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    assert_eq!(points.len(), 17);
    assert_eq!(points[0], [8.5, 21., 0.]);
    assert_eq!(points[8], [8., 22., 0.]);
    assert_eq!(points[16], [8.5, 23., 0.]);
    assert!(geometry_budgeted(&o, &m, &BTreeMap::new(), &mut 100, &mut budget).is_err());
    assert!(budget.exhausted);
    assert!(geometry(&o, &m, &BTreeMap::new(), &mut 0).is_err());
}

#[test]
fn added_profiles_resolve_only_same_major_target_segments() {
    for major in [24, 29, 28, 26] {
        let mut d = revised_placement_doc();
        for s in &mut d.segments {
            s.registry.major = major;
        }
        assert!(resolve(&d, &d.segments[0], 1, "DlSheetDlSegmentType", &mut 100).is_ok());
        d.segments[1].registry.major = if major == 28 { 29 } else { 28 };
        assert!(resolve(&d, &d.segments[0], 1, "DlSheetDlSegmentType", &mut 100).is_err());
    }
}

#[test]
fn saved_triangle_geometry_transforms_once_and_rejects_bad_topology() {
    let make = |points: Vec<f32>, indices: Vec<u32>| {
        observation(
            0,
            "stored_triangles_candidate",
            vec![
                ("triangle_vertices", FieldValue::F32(points)),
                ("triangle_indices", FieldValue::U32(indices)),
            ],
        )
    };
    let points = vec![0., 0., 0., 1., 0., 0., 0., 1., 0.];
    let o = make(points.clone(), vec![0, 1, 2]);
    let mut m = crate::assembly::IDENTITY;
    m[0][0] = 2.;
    m[0][3] = 10.;
    m[1][3] = -5.;
    let Some(DisplayGeometry::Triangles { vertices, indices }) =
        geometry(&o, &m, &BTreeMap::new(), &mut 100).unwrap()
    else {
        panic!()
    };
    assert_eq!(vertices, [[10., -5., 0.], [12., -5., 0.], [10., -4., 0.]]);
    assert_eq!(indices, [0, 1, 2]);
    for indices in [vec![0, 1, 3], vec![0, 0, 1], vec![0, 1]] {
        assert!(geometry(
            &make(points.clone(), indices),
            &m,
            &BTreeMap::new(),
            &mut 100
        )
        .is_err());
    }
    let mut nonplanar = points.clone();
    nonplanar[8] = 1.;
    assert!(geometry(
        &make(nonplanar, vec![0, 1, 2]),
        &m,
        &BTreeMap::new(),
        &mut 100
    )
    .is_err());
    let limits = DrawingLimits {
        max_polyline_points: 2,
        ..DrawingLimits::default()
    };
    let mut budget = DisplayBudget::new(&limits);
    assert!(geometry_budgeted(&o, &m, &BTreeMap::new(), &mut 100, &mut budget).is_err());
    assert!(geometry(&o, &m, &BTreeMap::new(), &mut 0).is_err());
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
    let f = fonts(&d, &mut 100, &mut vec![]).unwrap();
    assert!(!f.contains_key(&1));
    assert_eq!(f[&77].family, "Synthetic font");
    let mut rows = entry();
    let mut unknown = entry();
    unknown[0].1 = w(78);
    unknown[1].1 = FieldValue::U16(1);
    rows.extend(unknown);
    let mut zero = entry();
    zero[0].1 = w(79);
    zero[4].1 = FieldValue::F32(vec![0., 1.]);
    rows.extend(zero);
    rows.push(("font_next_id", w(80)));
    let d = document(vec![segment(
        "DlDirectorySegmentType",
        [4; 16],
        vec![observation(0, "font_table_candidate", rows)],
    )]);
    let mut diagnostics = vec![];
    let f = fonts(&d, &mut 100, &mut diagnostics).unwrap();
    assert_eq!(f.len(), 1);
    assert_eq!(f[&77].family, "Synthetic font");
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].starts_with("2 native font entries"));
    let mut unknown = entry();
    unknown[1].1 = FieldValue::U16(1);
    let mut rows = entry();
    rows.extend(unknown);
    rows.push(("font_next_id", w(78)));
    let d = document(vec![segment(
        "DlDirectorySegmentType",
        [4; 16],
        vec![observation(0, "font_table_candidate", rows)],
    )]);
    assert!(fonts(&d, &mut 100, &mut vec![]).is_err());
    let mut rows = entry();
    rows.extend(entry());
    rows.push(("font_next_id", w(78)));
    let d = document(vec![segment(
        "DlDirectorySegmentType",
        [4; 16],
        vec![observation(0, "font_table_candidate", rows)],
    )]);
    assert!(fonts(&d, &mut 100, &mut vec![]).is_err());
}

fn attach_attribute(doc: &mut DrawingInventory, ordinal: usize, attribute: PayloadObservation) {
    let attribute_id = attribute.record_ordinal as u32;
    let o = &mut doc.segments[1].observations[ordinal];
    o.fields.push(FieldObservation {
        name: "attribute_reference",
        value: w(attribute_id),
        source: source(),
    });
    doc.segments[1].observations.push(observation(
        attribute_id as usize - 1,
        "display_attributes_candidate",
        vec![("attribute_entry", w(attribute_id + 1))],
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
fn major28_hidden_curve_attribute_is_inherited_without_promoting_other_profiles() {
    for major in [23, 24, 26, 28, 29, 31] {
        for owner in [0, 1] {
            for value in [0, 1, 9] {
                let mut d = placement_doc();
                d.segments[1].registry.major = major;
                attach_attribute(
                    &mut d,
                    owner,
                    observation(
                        101,
                        "display_boolean_candidate",
                        vec![
                            ("attribute_mask", w(2)),
                            ("attribute_boolean", FieldValue::U8(value)),
                        ],
                    ),
                );
                for kind in ["DlSheetDlSegmentType", "DlSheetSmSegmentType"] {
                    d.segments[1].registry.kind = kind.into();
                    let segment = &d.segments[1];
                    let nodes = by_ordinal(segment, &mut 1000).unwrap();
                    let ordinals = nodes.keys().copied().collect();
                    let mut style = DisplayStyle::default();
                    for id in [0, 1] {
                        style = super::super::appearance::apply(
                            &d,
                            segment,
                            nodes[&id],
                            &nodes,
                            &ordinals,
                            &style,
                            &mut 1000,
                            &mut ResolveCache::default(),
                        )
                        .unwrap();
                    }
                    assert_eq!(style.sources.len(), 2);
                    if major == 28 && kind == "DlSheetDlSegmentType" && value == 0 {
                        assert!(!style.visible);
                        assert!(!style.unresolved.contains(&"boolean_mask_not_interpreted"));
                    } else {
                        assert!(style.visible);
                        assert!(style.unresolved.contains(&"boolean_mask_not_interpreted"));
                    }
                }
            }
        }
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
    let layer_fields = &mut d.segments[2].observations[0].fields;
    for (name, value) in [
        ("layer_scale", FieldValue::F64(vec![1.])),
        ("layer_flag_a", FieldValue::U8(0)),
        ("layer_flag_b", FieldValue::U8(1)),
        ("layer_flag_c", FieldValue::U8(0)),
    ] {
        layer_fields.push(FieldObservation {
            name,
            value,
            source: source(),
        });
    }
    let pinned: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/data/drawing-linetype-controls.json"
    ))
    .unwrap();
    // Golden nominal arrays captured independently from native entity overrides.
    // A non-unit binding scale remains unqualified for these layer patterns.
    for pattern in pinned["patterns"].as_array().unwrap().iter().skip(1) {
        d.segments[2].observations[0].fields[5].value =
            w(pattern["layer_pattern"].as_u64().unwrap() as u32);
        assert!(
            experimental_scene(&d, &Limits::default()).spaces[0].items[0]
                .style
                .dash
                .is_none()
        );
        d.segments[1].observations[3].fields[3].value = FieldValue::F32(vec![1.]);
        let result = experimental_scene(&d, &Limits::default());
        let actual = result.spaces[0].items[0].style.dash.as_ref().unwrap();
        let expected = pattern["nominal_dash_source_units"].as_array().unwrap();
        assert_eq!(actual.len(), expected.len());
        for (a, b) in actual.iter().zip(expected) {
            assert!((a - b.as_f64().unwrap()).abs() < 1e-12);
        }
        d.segments[1].observations[3].fields[3].value = FieldValue::F32(vec![2.]);
    }
    d.segments[2].observations[0].fields[5].value = w(28110);
    d.segments[1].observations[3].fields[3].value = FieldValue::F32(vec![1.]);
    d.segments[2].observations[0].fields[7].value = FieldValue::U8(1);
    assert_eq!(
        experimental_scene(&d, &Limits::default()).spaces[0].items[0]
            .style
            .dash,
        Some(vec![0.36, 0.09, 0.015, 0.09])
    );
    for major in [23, 24, 26, 28, 29] {
        for flag in [0, 1, 2] {
            d.segments[2].observations[0].fields[7].value = FieldValue::U8(flag);
            for pattern in [28101, 28107] {
                d.segments[2].observations[0].fields[5].value = w(pattern);
                let dash =
                    super::super::appearance::layer_dashes(&d.segments[2].observations[0], major)
                        .unwrap();
                let supported = pattern == 28101
                    && ((major == 26 && flag == 0) || (matches!(major, 28 | 29) && flag == 1));
                assert_eq!(dash.is_some(), supported);
                if supported {
                    let base = if flag == 0 { 0.038_f32 as f64 } else { 0.03 };
                    assert_eq!(dash, Some(vec![12. * base, 3. * base]));
                }
            }
        }
    }
    d.segments[2].observations[0].fields[7].value = FieldValue::U8(2);
    assert!(
        experimental_scene(&d, &Limits::default()).spaces[0].items[0]
            .style
            .dash
            .is_none()
    );
    d.segments[2].observations[0].fields[5].value = w(28100);
    d.segments[1].observations[3].fields[3].value = FieldValue::F32(vec![2.]);
    // Native major31 default sketch setters retain the layer width but
    // introduce a continuous override, even over a noncontinuous layer.
    attach_attribute(
        &mut d,
        1,
        observation(
            103,
            "stroke_override_candidate",
            vec![
                ("stroke_mask", w(8)),
                ("stroke_width", FieldValue::F32(vec![-1.])),
                ("stroke_flags", FieldValue::U16(0)),
                ("stroke_mode", FieldValue::U16(1)),
                ("stroke_pattern", FieldValue::U16(u16::MAX)),
                ("stroke_byte", FieldValue::U8(0)),
                ("stroke_list_type", FieldValue::U16(2)),
                ("stroke_dashes", FieldValue::F64(vec![])),
                (
                    "stroke_parameters",
                    FieldValue::F32(vec![-10000., 1., -0.038]),
                ),
                ("stroke_pattern_copy", w(u32::MAX)),
                ("stroke_kind", w(6)),
            ],
        ),
    );
    let inherited = experimental_scene(&d, &Limits::default());
    assert_eq!(inherited.spaces[0].items[0].style.width, Some(0.06));
    assert_eq!(inherited.spaces[0].items[0].style.dash, Some(vec![]));
    for (name, value) in [
        ("stroke_width", FieldValue::F32(vec![-2.])),
        ("stroke_kind", w(7)),
        ("stroke_pattern_copy", w(28102)),
        (
            "stroke_parameters",
            FieldValue::F32(vec![-10000., 2., -0.038]),
        ),
    ] {
        let stroke = d.segments[1].observations.last_mut().unwrap();
        let field = stroke.fields.iter_mut().find(|f| f.name == name).unwrap();
        let original = std::mem::replace(&mut field.value, value);
        assert!(experimental_scene(&d, &Limits::default()).spaces.is_empty());
        let stroke = d.segments[1].observations.last_mut().unwrap();
        stroke
            .fields
            .iter_mut()
            .find(|f| f.name == name)
            .unwrap()
            .value = original;
    }
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
    let small = DrawingLimits {
        max_reference_visits: 10,
        ..DrawingLimits::default()
    };
    assert!(stored_sheets_with_limits(&d, &Limits::default(), &small)
        .sheets
        .is_empty());
}

// Test conveniences use the public default ceilings, while production expansion
// shares one budget across all placements and sheets.
fn geometry(
    o: &PayloadObservation,
    m: &Matrix,
    fonts: &BTreeMap<u32, DisplayFont>,
    work: &mut usize,
) -> Result<Option<DisplayGeometry>> {
    geometry_budgeted(
        o,
        m,
        fonts,
        work,
        &mut DisplayBudget::new(&DrawingLimits::default()),
    )
}
fn owners<'a>(
    s: &'a SegmentInventory,
    work: &mut usize,
) -> Result<BTreeMap<usize, &'a PayloadObservation>> {
    owners_with_depth(s, work, DrawingLimits::default().max_nesting_depth).map(|d| d.nodes)
}

#[test]
fn expansion_budget_is_aggregate_and_output_writer_checks_before_append() {
    use std::io::Write;
    let drawing = DrawingLimits {
        max_display_items: 1,
        max_polyline_points: 2,
        max_output_bytes: 3,
        ..DrawingLimits::default()
    };
    let mut budget = DisplayBudget::new(&drawing);
    geometry_budgeted(
        &line(1, 0),
        &IDENTITY,
        &BTreeMap::new(),
        &mut 100,
        &mut budget,
    )
    .unwrap();
    assert!(geometry_budgeted(
        &line(1, 0),
        &IDENTITY,
        &BTreeMap::new(),
        &mut 100,
        &mut budget
    )
    .is_err());
    assert!(budget.exhausted);
    let mut out = drawing.output_buffer().unwrap();
    out.write_all(b"abc").unwrap();
    assert!(out.write_all(b"d").is_err());
    assert_eq!(out.into_string().unwrap(), "abc");
    assert!(DrawingLimits {
        max_sheets: usize::MAX,
        ..DrawingLimits::default()
    }
    .validate()
    .is_err());
}

#[test]
fn native_major24_color_mask_eight_does_not_enable_neighboring_profiles() {
    for major in [23, 24, 26, 28, 29, 31] {
        let mut d = placement_doc();
        for segment in &mut d.segments {
            segment.registry.major = major;
        }
        let mut rgba = vec![0.; 21];
        rgba[..4].copy_from_slice(&[1., 0., 1., 1.]);
        attach_attribute(
            &mut d,
            1,
            observation(
                101,
                "display_color_candidate",
                vec![
                    ("color_mask", w(8)),
                    ("color_rgba_parameters", FieldValue::F32(rgba)),
                ],
            ),
        );
        let s = experimental_scene(&d, &Limits::default());
        if major == 24 {
            assert_eq!(s.spaces[0].items.len(), 1);
            assert_eq!(s.spaces[0].items[0].style.rgba, Some([1., 0., 1., 1.]));
        } else {
            assert!(s.spaces[0].items.is_empty());
        }
    }
}

#[test]
fn unknown_color_omits_only_its_subtree_without_guessing_a_visible_style() {
    let mut d = placement_doc();
    d.segments[1].observations[0].fields[3].value = FieldValue::U32(vec![0x80000002, 0x80000003]);
    d.segments[1].observations.push(line(2, 0x80000001));
    attach_attribute(
        &mut d,
        1,
        observation(
            101,
            "display_color_candidate",
            vec![
                ("color_mask", w(4)),
                ("color_rgba_parameters", FieldValue::F32(vec![0.; 21])),
            ],
        ),
    );
    let s = experimental_scene(&d, &Limits::default());
    assert_eq!(s.spaces[0].items.len(), 1);
    assert_eq!(s.spaces[0].items[0].record_ordinal, 2);
    assert!(s.spaces[0]
        .omitted
        .iter()
        .any(|o| o.record_ordinal == 1 && o.reason == "display_color_mask_not_interpreted"));
}

#[test]
fn local_transformed_table_uses_local_slots_and_applies_matrix_once() {
    let mut d = placement_doc();
    d.segments[0].observations[0].fields[1].value = FieldValue::U32(vec![0x80000002]);
    let mut table = observation(
        1,
        "sheet_local_transformed_display_candidate",
        vec![
            ("display_reference", w(0x80000003)),
            ("references_unresolved", FieldValue::U32(vec![])),
        ],
    );
    let mut m = IDENTITY;
    m[0][3] = 12.;
    m[1][3] = -3.;
    add_matrix(&mut table, m);
    d.segments[0]
        .observations
        .extend([table, group(2, 0, &[0x80000004]), line(3, 0x80000003)]);
    let s = experimental_scene(&d, &Limits::default());
    let DisplayGeometry::Polyline { points } = &s.spaces[0].items[1].geometry else {
        panic!()
    };
    assert_eq!(points[0], [13., -1., 3.]);
    d.segments[0].observations[1].fields[0].value = w(3);
    assert!(experimental_scene(&d, &Limits::default()).spaces.is_empty());
}

#[test]
fn bitmap_placement_requires_a_local_typed_target_and_valid_bounds() {
    let make = || {
        let mut d = placement_doc();
        d.segments[0].observations[0].fields.extend([
            FieldObservation {
                name: "view_bitmap_reference",
                value: w(0x80000002),
                source: source(),
            },
            FieldObservation {
                name: "view_bitmap_bounds",
                value: FieldValue::F64(vec![-2., -1., -20., 2., 1., -10.]),
                source: source(),
            },
            FieldObservation {
                name: "view_name_unresolved",
                value: FieldValue::Utf16("View A".into()),
                source: source(),
            },
        ]);
        d.segments[0]
            .observations
            .push(observation(1, "stored_view_bitmap_candidate", vec![]));
        d
    };
    let d = make();
    let s = experimental_scene(&d, &Limits::default());
    let DisplayGeometry::Image {
        origin,
        u,
        v,
        format,
        reference,
    } = &s.spaces[0].items[1].geometry
    else {
        panic!()
    };
    assert_eq!(
        (*origin, *u, *v, *format, *reference),
        ([-2., 1., 0.], [4., 0., 0.], [0., -2., 0.], 3, 0x80000000)
    );
    assert_eq!(s.spaces[0].items[1].placement_record, 0);
    assert_eq!(s.spaces[0].views.len(), 1);
    assert_eq!(s.spaces[0].views[0].name, "View A");
    assert_eq!(s.spaces[0].views[0].image_reference, Some(0x80000000));
    let mut absent = make();
    absent.segments[0].observations[0].fields[4].value = w(0);
    absent.segments[0].observations[0].fields[5].value = FieldValue::F64(vec![f64::MAX; 6]);
    let no_cache = experimental_scene(&absent, &Limits::default());
    assert!(no_cache.spaces[0].views[0].cache_bounds.is_none());
    assert!(no_cache.spaces[0].views[0].image_reference.is_none());
    let limited = experimental_scene_with_limits(
        &d,
        &Limits::default(),
        &DrawingLimits {
            max_views: 0,
            ..DrawingLimits::default()
        },
    );
    assert!(limited.spaces.is_empty());
    assert!(limited.diagnostics.iter().any(|d| d.contains("view limit")));
    for mutation in 0..4 {
        let mut d = make();
        match mutation {
            0 => d.segments[0].observations[0].fields[4].value = w(2),
            1 => d.segments[0].observations[0].fields[4].value = w(0x80000009),
            2 => d.segments[0].observations[1].proposed_role = "stored_text_candidate",
            3 => {
                d.segments[0].observations[0].fields[5].value =
                    FieldValue::F64(vec![2., -1., 0., -2., 1., 0.])
            }
            _ => unreachable!(),
        }
        assert!(
            experimental_scene(&d, &Limits::default()).spaces.is_empty(),
            "mutation {mutation}"
        );
    }
}
