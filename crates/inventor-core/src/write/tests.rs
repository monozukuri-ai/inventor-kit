use super::*;
use acis_core::authoring::validate_box;
use std::io::{Cursor, Read};

#[test]
fn app_roots_include_the_keyless_parameter_collection() {
    let input = BoxDocument::new(BoxSpec::new((20., 30., 40.).into(), (0., 0., 0.).into()));
    let part = crate::part::inspect(
        &input.encode().unwrap(),
        "app-roots",
        &crate::Limits::default(),
    )
    .unwrap();
    let app = part
        .segments
        .iter()
        .find(|s| s.name == "PmAppSegment")
        .unwrap();
    let number = |name: &str| match &app
        .bulk_suffix_fields
        .iter()
        .find(|f| f.name == name)
        .unwrap()
        .value
    {
        crate::part::WireValue::U16(v) => u32::from(*v),
        crate::part::WireValue::U32(v) => *v,
        _ => panic!("unexpected App root field"),
    };
    // V43: omitting this table loses the keyless parameter collection during
    // native SaveAs, although the self-reader still sees its payload on input.
    assert_eq!(number("index_count"), 8, "App roots must be registered");
    let expected = [
        Some("f8d07626-42ec-5b63-9272-d59608ccabf0"),
        Some("345eb9b1-4861-b62a-7b08-f29a71f1aeef"),
        Some("7313fac3-4c44-8e67-7913-a3999a8976ab"),
        Some("61b5e2d1-11d4-3c78-8000-5a9a88fdf9c6"),
        None,
        Some("6759d870-11d2-7838-6000-94b70b02ecb0"),
        Some("f8a779f9-11d2-098f-c000-5a9a2378d04f"),
        Some("11fbeccd-11d1-256b-0008-ebbb21eddc09"),
    ];
    for (i, class) in expected.iter().enumerate() {
        let reference = number(&format!("index_{i}"));
        if let Some(class) = class {
            assert!(reference > 0 && reference <= app.records.len() as u32);
            assert_eq!(app.records[reference as usize - 1].type_id, *class);
        } else {
            assert_eq!(reference, 0);
        }
    }
    assert_eq!(number("pair_count"), 0);
}

#[test]
fn source_and_result_history_resolve_the_same_predecessor() {
    fn word(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }
    fn states(record: &crate::part::PartRecord) -> Vec<u32> {
        let kernel = record.kernel.as_ref().unwrap();
        let span = kernel
            .history
            .as_ref()
            .expect("creation history must exist");
        let bytes = &record.payload.0[span.start_offset - record.source.start_offset
            ..span.end_offset - record.source.start_offset];
        let mut cursor = 0;
        let mut states = Vec::new();
        while cursor < bytes.len() {
            let mut parts = Vec::new();
            loop {
                let tag = bytes[cursor];
                assert!(matches!(tag, 13 | 14));
                let len = usize::from(bytes[cursor + 1]);
                cursor += 2;
                parts.push(std::str::from_utf8(&bytes[cursor..cursor + len]).unwrap());
                cursor += len;
                if tag == 13 {
                    break;
                }
            }
            let name = parts.join("-");
            if name == "delta_state" {
                assert_eq!(bytes[cursor], 4);
                states.push(word(bytes, cursor + 1));
            }
            if matches!(name.as_str(), "history_stream" | "delta_state") {
                loop {
                    let tag = bytes[cursor];
                    cursor += 1;
                    match tag {
                        4 | 12 => cursor += 4,
                        11 => {}
                        17 => break,
                        _ => panic!("unexpected history token"),
                    }
                }
            }
        }
        states
    }
    let input = BoxDocument::new(BoxSpec::new((20., 30., 40.).into(), (0., 0., 0.).into()));
    let part = crate::part::inspect(
        &input.encode().unwrap(),
        "creation-chain",
        &crate::Limits::default(),
    )
    .unwrap();
    let dc = part
        .segments
        .iter()
        .find(|s| s.name == "PmDCSegment")
        .unwrap();
    let source = dc.records.iter().find(|r| r.kernel.is_some()).unwrap();
    let dc_helper = dc
        .records
        .iter()
        .find(|r| r.type_id == "452121b6-11d5-14d6-1000-61a6bba647b5")
        .unwrap();
    let predecessor = word(&dc_helper.payload.0, 26);
    let creation = word(&dc_helper.payload.0, 34);
    assert_eq!(states(source), [predecessor]);
    let source_footer =
        source.kernel.as_ref().unwrap().source.end_offset - source.source.start_offset;
    assert_eq!(word(&source.payload.0, source_footer + 5), predecessor);
    let brep = part
        .segments
        .iter()
        .find(|s| s.name == "PmBRepSegment")
        .unwrap();
    let result = brep.records.iter().find(|r| r.kernel.is_some()).unwrap();
    assert_eq!(states(result), [creation, predecessor]);
    let helper = brep
        .records
        .iter()
        .find(|r| r.type_id == "f78b08d5-4675-44a3-3532-fa8e4d94abf5")
        .unwrap();
    let bytes = &helper.payload.0;
    assert_eq!(word(bytes, 18), 2);
    assert_eq!([word(bytes, 30), word(bytes, 34)], [predecessor, creation]);
    assert_eq!(word(bytes, 42), 2);
    for (offset, state) in [(54, predecessor), (62, creation)] {
        assert_eq!(word(bytes, offset), state);
        let child = &brep.records[(word(bytes, offset + 4) - 1) as usize];
        assert_eq!(word(&child.payload.0, 4), state);
        assert_eq!(
            word(&child.payload.0, 0),
            0x8000_0000 | (helper.ordinal as u32 + 1)
        );
    }
    let terminal = &brep.records[(word(bytes, 70) - 1) as usize];
    assert_eq!(word(&terminal.payload.0, 4), u32::MAX);
}

#[test]
fn initial_result_history_resolves_the_base_feature_creation_state() {
    fn word(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }
    fn name(bytes: &[u8], cursor: &mut usize) -> String {
        let mut parts = Vec::new();
        loop {
            let tag = bytes[*cursor];
            assert!(matches!(tag, 13 | 14));
            let len = usize::from(bytes[*cursor + 1]);
            *cursor += 2;
            parts.push(std::str::from_utf8(&bytes[*cursor..*cursor + len]).unwrap());
            *cursor += len;
            if tag == 13 {
                return parts.join("-");
            }
        }
    }
    let input = BoxDocument::new(BoxSpec::new((20., 30., 40.).into(), (0., 0., 0.).into()));
    let part = crate::part::inspect(
        &input.encode().unwrap(),
        "initial-creation-state",
        &crate::Limits::default(),
    )
    .unwrap();
    let dc = part
        .segments
        .iter()
        .find(|s| s.name == "PmDCSegment")
        .unwrap();
    let dc_helper = dc
        .records
        .iter()
        .find(|r| r.type_id == "452121b6-11d5-14d6-1000-61a6bba647b5")
        .unwrap();
    // This is the initial base-feature anchor, not the latest state after edits.
    // Native V40 retained this value and an empty map through pre-edit SaveAs.
    let creation_state = word(&dc_helper.payload.0, 34);
    assert_eq!(creation_state, 3);
    let brep = part
        .segments
        .iter()
        .find(|s| s.name == "PmBRepSegment")
        .unwrap();
    let carrier = brep.records.iter().find(|r| r.kernel.is_some()).unwrap();
    let kernel = carrier.kernel.as_ref().unwrap();
    let history = kernel.history.as_ref().unwrap();
    let bytes = &carrier.payload.0[history.start_offset - carrier.source.start_offset
        ..history.end_offset - carrier.source.start_offset];
    let mut cursor = 0;
    assert_eq!(name(bytes, &mut cursor), "Begin-of-ASM-History-Data");
    assert_eq!(name(bytes, &mut cursor), "history_stream");
    assert_eq!(bytes[cursor], 4);
    assert_eq!(word(bytes, cursor + 1), creation_state);
    assert_eq!(bytes[cursor + 5], 4);
    assert_eq!(word(bytes, cursor + 6), creation_state);
    cursor += 8 * 5;
    assert_eq!(bytes[cursor], 17);
    cursor += 1;
    assert_eq!(name(bytes, &mut cursor), "delta_state");
    assert_eq!(bytes[cursor], 4);
    assert_eq!(word(bytes, cursor + 1), creation_state);
    let footer = kernel.source.end_offset - carrier.source.start_offset;
    assert_eq!(word(&carrier.payload.0, footer + 5), creation_state);
    let helper_ref = word(&carrier.payload.0, footer + 10);
    assert_eq!(helper_ref & 0x8000_0000, 0x8000_0000);
    let helper = &brep.records[((helper_ref & 0x7fff_ffff) - 1) as usize];
    assert_eq!(helper.type_id, "f78b08d5-4675-44a3-3532-fa8e4d94abf5");
    assert_eq!(word(&helper.payload.0, 34), creation_state);
    assert_eq!(word(&helper.payload.0, 62), creation_state);
    let current = &brep.records[(word(&helper.payload.0, 66) - 1) as usize];
    let terminal = &brep.records[(word(&helper.payload.0, 70) - 1) as usize];
    assert_eq!(word(&current.payload.0, 4), creation_state);
    assert_eq!(word(&terminal.payload.0, 4), u32::MAX);
    assert_eq!(word(&current.payload.0, 0), helper_ref);
    assert_eq!(word(&terminal.payload.0, 0), helper_ref);
}

#[test]
fn dc_helper_kernel_reference_resolves_to_the_emitted_source_sab() {
    let input = BoxDocument::new(BoxSpec::new((20., 30., 40.).into(), (0., 0., 0.).into()));
    let part = crate::part::inspect(
        &input.encode().unwrap(),
        "dc-source-reference",
        &crate::Limits::default(),
    )
    .unwrap();
    let dc = part
        .segments
        .iter()
        .find(|segment| segment.name == "PmDCSegment")
        .unwrap();
    let helper = dc
        .records
        .iter()
        .find(|record| record.type_id == "452121b6-11d5-14d6-1000-61a6bba647b5")
        .unwrap();
    // Native controls use this qualified local ordinal for the source SAB,
    // independently of the helper's declared target_class in the profile.
    let reference = u32::from_le_bytes(helper.payload.0[22..26].try_into().unwrap());
    assert_eq!(reference & 0x8000_0000, 0x8000_0000);
    let ordinal = usize::try_from((reference & 0x7fff_ffff) - 1).unwrap();
    let target = dc
        .records
        .iter()
        .find(|record| record.ordinal == ordinal)
        .unwrap();
    assert_eq!(target.type_id, "f645595c-11d5-1333-1000-60a6bba647b5");
    assert!(target.kernel.is_some());
}

#[test]
fn dc_helper_rejects_a_matching_class_that_is_not_a_source_carrier() {
    let input = BoxDocument::new(BoxSpec::new((20., 30., 40.).into(), (0., 0., 0.).into()));
    let graph = document::Graph::new(
        profile::Profile::load().unwrap(),
        &input,
        input.shape.build().unwrap(),
    )
    .unwrap();
    let mut helper = graph.profile.0["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["generation_contract"] == "dc-helper")
        .unwrap()
        .clone();
    let reference = helper["recipe"]["fields"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|field| field["name"] == "kernel_reference")
        .unwrap();
    reference["target_role"] = "90874d91-11d0-d1f8-0008-cabc0663dc09/key:35".into();
    reference["target_class"] = "90874d91-11d0-d1f8-0008-cabc0663dc09".into();
    let error = graph.recipe(&helper).unwrap_err();
    assert!(error.to_string().contains("not a source carrier"));
}

#[test]
fn supporting_planes_are_unbounded_and_trim_edges_stay_finite() {
    use acis_core::{Entity, ParameterRange};
    let input = BoxDocument::new(BoxSpec::new((7., 13., 2.).into(), (100., -50., 7.).into()));
    let document = crate::read(
        &input.encode().unwrap(),
        "support-domains",
        &crate::Limits::default(),
    )
    .unwrap();
    let mut planes = 0;
    let mut edges = 0;
    for entity in document.model.unwrap().entities() {
        match entity {
            Entity::PlaneSurface(plane) => {
                let infinite = Some(ParameterRange {
                    lower: None,
                    upper: None,
                });
                assert_eq!(plane.u_range, infinite);
                assert_eq!(plane.v_range, infinite);
                planes += 1;
            }
            Entity::Edge(edge) => {
                assert!(edge.start_parameter.unwrap().is_finite());
                assert!(edge.end_parameter.unwrap() > edge.start_parameter.unwrap());
                edges += 1;
            }
            _ => {}
        }
    }
    assert_eq!((planes, edges), (6, 12));
}

#[test]
fn both_carriers_label_the_body_and_all_faces_for_document_references() {
    let input = BoxDocument::new(BoxSpec::new((20., 30., 40.).into(), (0., 0., 0.).into()));
    let bytes = input.encode().unwrap();
    let part =
        crate::part::inspect(&bytes, "face-reference-labels", &crate::Limits::default()).unwrap();
    let kernels: Vec<_> = part
        .segments
        .iter()
        .flat_map(|s| &s.records)
        .filter_map(|r| r.kernel.as_ref())
        .collect();
    assert_eq!(kernels.len(), 2);
    for kernel in kernels {
        assert_eq!(kernel.custom_attributes.len(), 7);
        assert_eq!(
            kernel
                .custom_attributes
                .iter()
                .filter(|a| a.strings.iter().any(|s| s == "INV_NMX_GEN_TAG"))
                .count(),
            6
        );
        assert_eq!(
            kernel
                .custom_attributes
                .iter()
                .filter(|a| a.strings.iter().any(|s| s == "INV_MIX_CRE_ENTITY_QUALITY"))
                .count(),
            1
        );
    }
}

#[test]
fn document_labels_bind_each_original_entity_and_retain_its_producer_id() {
    use super::kernel_labels::History;
    use acis_core::{AcisValue, Entity, EntityRef, NULL_REF};
    let shape = BoxSpec::new((7., 13., 2.).into(), (100., -50., 7.).into())
        .build()
        .unwrap();
    let baseline_bytes = kernel_labels::geometry(&shape).unwrap();
    let baseline = acis_core::sab::parse_sab(
        &baseline_bytes,
        "label-ownership",
        &acis_core::sab::SabLimits::default(),
    )
    .unwrap();
    for history in [History::Source, History::BaseFeature] {
        let bytes = kernel_labels::encode(&shape, history).unwrap();
        let parsed = acis_core::sab::parse_sab(
            &bytes,
            "label-ownership",
            &acis_core::sab::SabLimits::default(),
        )
        .unwrap();
        let entities = parsed.model.entities();
        assert!(parsed.history.is_some());
        let mut owners = std::collections::BTreeSet::new();
        for original in baseline.model.entities() {
            let output = &entities[original.index()];
            assert_eq!(output.raw().entity_id, original.raw().entity_id);
            if !matches!(original, Entity::Body(_) | Entity::Face(_)) {
                assert_eq!(output, original);
                continue;
            }
            let label = &entities[usize::try_from(output.raw().attributes.0).unwrap()];
            assert_eq!(label.raw().type_name, "ATTRIB_CUSTOM-attrib");
            assert!(owners.insert(label.index()));
            assert_eq!(label.raw().attributes, NULL_REF);
            let values = &label.raw().values;
            assert_eq!(
                &values[..3],
                &[
                    AcisValue::Reference(NULL_REF),
                    AcisValue::Reference(NULL_REF),
                    AcisValue::Reference(EntityRef(output.index() as i64))
                ]
            );
            let (name, tail) = if matches!(output, Entity::Body(_)) {
                ("INV_MIX_CRE_ENTITY_QUALITY", vec![1, 44, 1])
            } else {
                let face_id: i32 = output.raw().entity_id.as_ref().unwrap().try_into().unwrap();
                ("INV_NMX_GEN_TAG", vec![1, 44, 1, 13, -1, face_id, 0, 132])
            };
            assert_eq!(values[3], AcisValue::String(name.into()));
            assert_eq!(
                &values[4..],
                tail.into_iter()
                    .map(|v: i32| AcisValue::Integer(v.into()))
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(owners.len(), 7);
    }
}

#[test]
fn boxes_roundtrip_geometry_graph_and_fresh_identity() {
    let mut identities = std::collections::BTreeSet::new();
    for (size, origin) in [
        ((20., 30., 40.), (0., 0., 0.)),
        ((7., 13., 2.), (100., -50., 7.)),
        ((1., 1., 1.), (-2., -3., -4.)),
        ((125., 3.5, 0.8), (4.2, 5.3, 6.4)),
        ((20., 30., 40.), (0., 0., 0.)),
    ] {
        let mut input = BoxDocument::new(BoxSpec::new(size.into(), origin.into()));
        input.document_name = "生成した部品 Ω".into();
        input.feature_name = "基礎形状".into();
        input.body_name = "箱".into();
        let bytes = input.encode().unwrap();
        let limits = crate::Limits::default();
        let doc = crate::read(&bytes, "roundtrip", &limits).unwrap();
        let model = doc.model.as_ref().unwrap();
        // AuthoredBox validates finite construction bounds. For its validator,
        // restore only those ranges, keeping all decoded coordinates, normals,
        // topology and curve parameters. Native support domains and document
        // labels have separate assertions.
        let authored = input.shape.build().unwrap();
        let mut geometry: Vec<_> = model.entities().iter().take(86).cloned().collect();
        for entity in &mut geometry {
            match entity {
                acis_core::Entity::Body(e) => e.raw.attributes = acis_core::NULL_REF,
                acis_core::Entity::Face(e) => e.raw.attributes = acis_core::NULL_REF,
                acis_core::Entity::PlaneSurface(e) => {
                    assert_eq!(
                        e.u_range,
                        Some(acis_core::ParameterRange {
                            lower: None,
                            upper: None
                        })
                    );
                    assert_eq!(e.v_range, e.u_range);
                    let acis_core::Entity::PlaneSurface(original) =
                        &authored.model().entities()[e.raw.index]
                    else {
                        panic!("support plane identity changed")
                    };
                    assert_eq!(
                        (e.origin, e.normal, e.u_direction, e.reverse_v),
                        (
                            original.origin,
                            original.normal,
                            original.u_direction,
                            original.reverse_v
                        )
                    );
                    e.u_range = original.u_range;
                    e.v_range = original.v_range;
                }
                _ => {}
            }
        }
        let active = acis_core::AcisModel::new(model.metadata().clone(), geometry, vec![]).unwrap();
        let valid = validate_box(&active).unwrap();
        assert!((valid.volume_mm3 - size.0 * size.1 * size.2).abs() < 1e-6);
        for (a, b) in [valid.minimum_mm.x, valid.minimum_mm.y, valid.minimum_mm.z]
            .into_iter()
            .zip([origin.0, origin.1, origin.2])
        {
            assert!((a - b).abs() < 1e-10);
        }
        let part = crate::part::inspect(&bytes, "roundtrip", &limits).unwrap();
        // Every emitted external object must have a real segment. Declaring
        // GUIDs for absent caches passes framing but fails normal native Open.
        let emitted: std::collections::BTreeSet<_> =
            part.segments.iter().map(|s| s.id.as_str()).collect();
        for segment in &part.segments {
            let objects = segment
                .meta_sections
                .iter()
                .find(|s| s.number == 7)
                .unwrap();
            for row in objects.bytes.0.chunks_exact(32) {
                let target = uuid::Uuid::from_bytes_le(row[..16].try_into().unwrap()).to_string();
                assert!(
                    emitted.contains(target.as_str()),
                    "absent external target: {target}"
                );
            }
        }
        assert_eq!(part.status, "framed");
        assert!(part.diagnostics.is_empty());
        assert_eq!(part.revisions.len(), 565);
        let dc = part
            .segments
            .iter()
            .find(|s| s.name == "PmDCSegment")
            .unwrap();
        let root = dc
            .records
            .iter()
            .find(|r| r.type_id == "90874d63-11d0-d1f8-0008-cabc0663dc09")
            .unwrap();
        // The accepted fixed-profile layout places the DC root at header key
        // 1, independently of its record ordinal (which is 2).
        assert_eq!(root.ordinal, 2);
        assert!(root
            .fields
            .iter()
            .any(|f| f.name == "header_id" && matches!(f.value, crate::part::WireValue::U16(1))));
        let label_class = guid("0ca9112d-11d0-838b-0008-19b00524dc09").unwrap();
        let label_context = dc.meta_sections.iter().find(|s| s.number == 9).unwrap();
        assert_eq!(label_context.count_or_discriminator, 1);
        assert_eq!(&label_context.bytes.0[..3], &[1, 0, 1]);
        assert_eq!(&label_context.bytes.0[3..], &label_class);
        // Cross-check the Meta protocol against the independently decoded DC
        // label records. Per-document identities change in every iteration.
        assert!(dc.records.iter().flat_map(|r| &r.fields).any(|f| {
            f.name == "label_class"
                && matches!(&f.value, crate::part::WireValue::Guid(id)
                    if uuid::Uuid::parse_str(id).unwrap().to_bytes_le() == label_class)
        }));
        assert!(part.database_fields.iter().any(|f| {
            f.name == "database_id"
                && matches!(&f.value, crate::part::WireValue::Guid(id)
                    if uuid::Uuid::parse_str(id).unwrap().to_bytes_le() != label_class)
        }));
        let label = dc
            .records
            .iter()
            .find(|r| r.type_id == "56970dfa-11d4-2cdf-c000-9b9645df724f")
            .unwrap();
        // M1 V60 binds this field to source faces plus closed edges. None of
        // the box's 12 straight edges has coincident start/end vertices.
        assert_eq!(
            u32::from_le_bytes(label.payload.0[30..34].try_into().unwrap()),
            6
        );
        let points: Vec<_> = active
            .entities()
            .iter()
            .filter_map(|e| match e {
                acis_core::entities::Entity::Point(p) => {
                    Some([p.location.x, p.location.y, p.location.z])
                }
                _ => None,
            })
            .collect();
        let graphics = part
            .segments
            .iter()
            .find(|s| s.name == "PmGraphicsSegment")
            .unwrap();
        let result_cache = part
            .segments
            .iter()
            .find(|s| s.name == "PmResultSegment")
            .unwrap();
        let bounds = &result_cache.records[0].payload.0[58..106];
        for (i, number) in bounds.chunks_exact(8).enumerate() {
            let expected = points.iter().map(|p| p[i % 3]).fold(
                if i < 3 {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                },
                if i < 3 { f64::min } else { f64::max },
            );
            assert_eq!(
                f64::from_le_bytes(number.try_into().unwrap()).to_bits(),
                expected.to_bits()
            );
        }
        for draw in graphics
            .records
            .iter()
            .filter(|r| r.type_id == "5194e9a3-11d2-289b-6000-6ab72c39cdb0")
        {
            for (i, number) in draw.payload.0[51..99].chunks_exact(8).enumerate() {
                let coordinate = f64::from_le_bytes(number.try_into().unwrap());
                assert!(
                    points
                        .iter()
                        .any(|p| p[i % 3].to_bits() == coordinate.to_bits()),
                    "Graphics bounds must use the emitted SAB points"
                );
            }
        }
        for s in &part.segments {
            assert_eq!(s.status, "framed");
            assert_eq!(s.stream_token_matches_id, Some(true));
            assert!(identities.insert(s.id.clone()));
            // The 58-file native development corpus has 0 for these four
            // owners and 1 for the two owners without external object rows.
            // A universal 1 is accepted by our reader but differs from the
            // native registration contract.
            let expected_version_state = match s.name.as_str() {
                "PmGraphicsSegment" | "PmDCSegment" | "PmBrowserSegment" | "PmAppSegment" => 0,
                "PmBRepSegment" | "DesignViewSegment" | "NBNotebookSegment" | "PmResultSegment" => {
                    1
                }
                other => panic!("unexpected owner: {other}"),
            };
            let version_state = s
                .registry_fields
                .iter()
                .find(|f| f.name == "version_state")
                .unwrap();
            assert!(
                matches!(version_state.value, crate::part::WireValue::U32(v) if v == expected_version_state),
                "{} registry version_state must match the native owner contract",
                s.name
            );
            let revisions = s.meta_sections.iter().find(|s| s.number == 2).unwrap();
            assert_eq!(revisions.count_or_discriminator, 2);
            let rows = &revisions.bytes.0;
            assert_eq!(rows.len(), 20);
            assert_eq!(&rows[..10], &[0; 10], "Meta2 retains the zero anchor");
            let current = u32::from_le_bytes(rows[10..14].try_into().unwrap()) as usize;
            assert_eq!(current, part.revisions.len() - 1);
            let current_id = s
                .registry_fields
                .iter()
                .find(|f| f.name == "revision_id")
                .unwrap();
            match &current_id.value {
                crate::part::WireValue::Guid(id) => assert_eq!(&part.revisions[current].id, id),
                value => panic!("unexpected revision identity: {value:?}"),
            }
        }
        let kernels: Vec<_> = part
            .segments
            .iter()
            .flat_map(|s| &s.records)
            .filter_map(|r| r.kernel.as_ref())
            .collect();
        assert_eq!(kernels.len(), 2);
        assert_eq!(kernels.iter().filter(|k| k.history.is_some()).count(), 2);
        for k in kernels {
            assert_eq!(k.save_version, 23200);
            assert_eq!(k.bodies.len(), 1);
            assert_eq!(k.type_counts["face"], 6);
            assert_eq!(k.type_counts["edge"], 12);
            assert_eq!(k.type_counts["vertex"], 8);
            assert_eq!(k.custom_attributes.len(), 7);
        }
        let mut c = ::cfb::CompoundFile::open(Cursor::new(&bytes)).unwrap();
        assert_eq!(c.version(), ::cfb::Version::V3);
        assert_eq!(c.walk().filter(|e| e.is_stream()).count(), 22);
        let paths: std::collections::BTreeSet<_> = c
            .walk()
            .filter(|e| e.is_stream())
            .map(|e| e.path().to_string_lossy().into_owned())
            .collect();
        assert!(paths.contains("/RSeStorage/RSeEmbeddings/DatabaseInterfaces"));
        assert!(!paths.contains("/DatabaseInterfaces"));
        // Native controls and accepted M1 constructors declare each complete
        // frame's expanded size. A streaming self-reader also accepts an absent
        // size, so geometry roundtrips alone do not cover this wire contract.
        for segment in &part.segments {
            let form = segment
                .meta_fields
                .iter()
                .find(|f| f.name == "body_form")
                .unwrap();
            let bulk_path = format!(
                "/RSeStorage/B{}",
                segment.stream_token_observed.as_ref().unwrap()
            );
            for (path, offset) in [
                (form.source.stream.as_str(), form.source.end_offset),
                (bulk_path.as_str(), 18),
            ] {
                let mut encoded = Vec::new();
                c.open_stream(path)
                    .unwrap()
                    .read_to_end(&mut encoded)
                    .unwrap();
                let frame = &encoded[offset..];
                let decoded = zstd::stream::decode_all(frame).unwrap();
                if path == form.source.stream.as_str() {
                    // These are aliases of the emitted suffix, registry and
                    // outer header, not retained native preset numbers.
                    let field_u32 = |fields: &[crate::part::WireField], name: &str| match &fields
                        .iter()
                        .find(|f| f.name == name)
                        .unwrap()
                        .value
                    {
                        crate::part::WireValue::U16(v) => *v as u32,
                        crate::part::WireValue::U32(v) => *v,
                        value => panic!("unexpected wire field: {value:?}"),
                    };
                    let cache = matches!(
                        segment.name.as_str(),
                        "NBNotebookSegment" | "PmResultSegment"
                    );
                    assert_eq!(
                        u16::from_le_bytes(decoded[4..6].try_into().unwrap()) as u32,
                        if cache {
                            0
                        } else {
                            field_u32(&segment.bulk_suffix_fields, "index_count")
                        },
                        "Meta suffix alias must match the owner profile"
                    );
                    assert_eq!(
                        u32::from_le_bytes(decoded[6..10].try_into().unwrap()),
                        if cache {
                            field_u32(&segment.bulk_suffix_fields, "index_count")
                        } else {
                            field_u32(&segment.registry_fields, "state_word_5")
                        },
                        "Meta slot limit must match the owner profile"
                    );
                    assert_eq!(
                        u32::from_le_bytes(decoded[10..14].try_into().unwrap()) as usize,
                        offset + 10,
                        "Meta offset alias must follow emitted outer header"
                    );
                }
                assert_eq!(
                    zstd::zstd_safe::get_frame_content_size(frame).unwrap(),
                    Some(decoded.len() as u64),
                    "{} must declare the full expanded size",
                    path
                );
                assert_eq!(
                    zstd::zstd_safe::find_frame_compressed_size(frame).unwrap(),
                    frame.len()
                );
            }
        }
    }
}

#[test]
fn fixed_camera_values_keep_declared_precision() {
    let profile = profile::Profile::load().unwrap();
    let camera = array(&profile.0, "records")
        .unwrap()
        .iter()
        .find(|r| {
            r["owner"] == "DesignViewSegment"
                && r["role"] == "08823621-11d2-b7c6-a000-3ba976d456c9/profile-key:3"
        })
        .unwrap();
    let field = array(&camera["recipe"], "fields")
        .unwrap()
        .iter()
        .find(|f| f["name"] == "camera_number_3")
        .unwrap();
    // Exact M1 V55 native preset; JSON decoding must preserve its f64 value.
    assert_eq!(
        field["value"].as_f64().unwrap().to_bits(),
        0.9807762217264311_f64.to_bits()
    );
}

#[test]
fn invalid_semantic_input_is_rejected_before_emission() {
    for size in [
        (0., 3., 4.),
        (-1., 3., 4.),
        (f64::NAN, 3., 4.),
        (1e7, 3., 4.),
    ] {
        assert!(
            BoxDocument::new(BoxSpec::new(size.into(), (0., 0., 0.).into()))
                .encode()
                .is_err()
        );
    }
    let mut doc = BoxDocument::new(BoxSpec::new((20., 30., 40.).into(), (0., 0., 0.).into()));
    for name in [String::new(), "bad\0name".into(), "x".repeat(256)] {
        doc.body_name = name;
        assert!(doc.encode().is_err());
    }
}

#[test]
fn unknown_profile_policy_and_unresolved_reference_fail_closed() {
    let input = BoxDocument::new(BoxSpec::new((20., 30., 40.).into(), (0., 0., 0.).into()));
    for suffix in ["key:0", "key:65535", "key:5", "unknown"] {
        let mut p = profile::Profile::load().unwrap();
        let root = p.0["records"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["class"] == "90874d63-11d0-d1f8-0008-cabc0663dc09")
            .unwrap();
        root["role"] = format!("90874d63-11d0-d1f8-0008-cabc0663dc09/{suffix}").into();
        // Reserved, duplicate and unregistered keys must fail before encoding.
        assert!(document::Graph::new(p, &input, input.shape.build().unwrap()).is_err());
    }
    let mut g = document::Graph::new(
        profile::Profile::load().unwrap(),
        &input,
        input.shape.build().unwrap(),
    )
    .unwrap();
    let mut rec = g.profile.0["records"][0].clone();
    rec["recipe"]["fields"][0]["policy"] = "raw_payload".into();
    assert!(g.recipe(&rec).is_err());
    g.allocations.remove(&(
        rec["owner"].as_str().unwrap().into(),
        rec["role"].as_str().unwrap().into(),
    ));
    assert!(g.recipe(&g.profile.0["records"][0]).is_err());
}
