use super::*;
use crate::{rse, Limits};
use std::io::{Cursor, Read, Write};

type Cfb = cfb::CompoundFile<Cursor<Vec<u8>>>;
const TYPE_A: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const TYPE_B: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17];
fn word(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
fn text(out: &mut Vec<u8>, s: &str) {
    word(out, s.len() as u32);
    out.extend(s.as_bytes());
}
fn wide(out: &mut Vec<u8>, s: &str) {
    word(out, s.encode_utf16().count() as u32);
    for c in s.encode_utf16() {
        out.extend(c.to_le_bytes());
    }
}
fn put(c: &mut Cfb, path: &str, data: &[u8]) {
    c.create_stream(path).unwrap().write_all(data).unwrap();
}
fn get(c: &mut Cfb, path: &str) -> Vec<u8> {
    let mut b = vec![];
    c.open_stream(path).unwrap().read_to_end(&mut b).unwrap();
    b
}
fn zstd(data: &[u8]) -> Vec<u8> {
    zstd::stream::encode_all(data, 1).unwrap()
}
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}
fn mutate(data: &[u8], edit: impl FnOnce(&mut Cfb)) -> Vec<u8> {
    let mut c = cfb::CompoundFile::open(Cursor::new(data.to_vec())).unwrap();
    edit(&mut c);
    c.into_inner().into_inner()
}

fn registry(entries: &[(&str, u8, [u8; 16], &str)]) -> Vec<u8> {
    let mut out = vec![];
    word(&mut out, entries.len() as u32);
    for (name, major, id, kind) in entries {
        wide(&mut out, name);
        out.extend(id);
        out.extend([0; 20]);
        word(&mut out, 0);
        out.extend([0; 24]);
        wide(&mut out, kind);
        out.extend([0; 8]);
        out.extend([0, 0, *major, 0, 0, 0, 0, 0]);
        out.extend([0; 4]);
    }
    out.extend([0; 12]);
    out
}
fn metadata(name: &str, id: [u8; 16], types: &[[u8; 16]]) -> Vec<u8> {
    let mut body = vec![0; 14];
    word(&mut body, 3);
    for n in [0x80000003, 0, 0x80000002] {
        word(&mut body, n);
    }
    word(&mut body, 16);
    for _ in 0..2 {
        word(&mut body, 0);
        word(&mut body, 4);
    }
    word(&mut body, types.len() as u32);
    for t in types {
        body.extend(t);
        body.extend([0; 12]);
    }
    word(&mut body, 4 + types.len() as u32 * 28);
    for n in 5..=10 {
        word(&mut body, if n == 5 { 0xff00ff00 } else { 0 });
        word(&mut body, 4);
    }
    word(&mut body, 18);
    body.extend([0; 88]);
    let mut out = vec![];
    text(&mut out, "RSe Meta Stream Version 8");
    out.extend(8u16.to_le_bytes());
    out.extend([0; 16]);
    wide(&mut out, name);
    out.extend(id);
    out.extend([0; 12]);
    text(&mut out, "");
    text(&mut out, "");
    out.push(0);
    out.extend(zstd(&body));
    out
}
fn body() -> Vec<u8> {
    let mut out = vec![];
    word(&mut out, 0xaabbcc00);
    out.extend(b"abc");
    word(&mut out, 3);
    out.push(1);
    word(&mut out, 0);
    out.extend(6u16.to_le_bytes());
    out.extend(0x3000u16.to_le_bytes());
    word(&mut out, 2);
    out.extend([0; 8]);
    for name in ["self", "self"] {
        text(&mut out, name);
        word(&mut out, 0);
    }
    word(&mut out, 1);
    out.extend(b"xy");
    word(&mut out, 2);
    out.push(0);
    word(&mut out, u32::MAX);
    out.extend(b"opaque tail");
    out
}
fn bulk(data: &[u8]) -> Vec<u8> {
    let mut b = profile::BULK_HEADER.to_vec();
    b.extend(zstd(data));
    b
}
fn fixture() -> Vec<u8> {
    let mut c = Cfb::create(Cursor::new(vec![])).unwrap();
    c.set_storage_clsid("/", "bbf9fdf1-52dc-11d0-8c04-0800090be8ec".parse().unwrap())
        .unwrap();
    c.create_storage_all("/RSeStorage/V1").unwrap();
    let mut db = vec![0; 16];
    word(&mut db, 31);
    db.extend([0; 32]);
    wide(&mut db, "");
    put(&mut c, "/RSeStorage/V1/RSeDb", &db);
    put(
        &mut c,
        "/RSeStorage/RSeSegInfo",
        &registry(&[("DLSheet999DLSegment", 31, [1; 16], "DlSheetDlSegmentType")]),
    );
    put(
        &mut c,
        "/RSeStorage/Mone",
        &metadata("DLSheet999DLSegment", [1; 16], &[TYPE_A, TYPE_B]),
    );
    put(&mut c, "/RSeStorage/Bone", &bulk(&body()));
    c.into_inner().into_inner()
}
fn read(data: &[u8]) -> DrawingInventory {
    inspect(data, "renamed.any", &Limits::default()).unwrap()
}
fn has(doc: &DrawingInventory, code: &str) -> bool {
    doc.diagnostics
        .iter()
        .chain(doc.segments.iter().flat_map(|s| &s.diagnostics))
        .any(|d| d.code == code)
}
fn decoded_bulk(c: &mut Cfb) -> Vec<u8> {
    let b = get(c, "/RSeStorage/Bone");
    rse::inflate(&b[18..], 65536).unwrap().0
}

#[test]
fn full_types_ordinals_spans_and_unresolved_references_are_preserved() {
    let data = fixture();
    let d = read(&data);
    assert_eq!(d.status, "framed_subset");
    assert_eq!(d.sheet_count, None);
    assert_eq!(d.drawing_semantics, "not_decoded");
    let s = &d.segments[0];
    assert_eq!(
        s.records.iter().map(|r| r.ordinal).collect::<Vec<_>>(),
        [0, 2]
    );
    assert_ne!(s.records[0].type_id, s.records[1].type_id);
    assert_eq!(s.records[0].selector_word, 0xaabbcc00);
    assert_eq!(s.records[0].source.start_offset, 4);
    assert_eq!(s.records[0].source.end_offset, 7);
    assert_eq!(s.records[0].source.byte_domain, "inflated_stream");
    assert_eq!(s.records[0].source.stream, "/RSeStorage/Bone");
    assert_eq!(s.records[0].meta_block_source.start_offset, 18);
    assert_eq!(s.records[1].meta_block_source.start_offset, 26);
    assert_eq!(s.records[0].references.len(), 2);
    assert!(s.records[0]
        .references
        .iter()
        .all(|r| r.raw_value == 0 && r.status == "unresolved_not_followed"));
    assert!(s
        .opaque_regions
        .iter()
        .any(|r| r.reason == "bulk_suffix_not_interpreted"));
    let mut c = Cfb::open(Cursor::new(data)).unwrap();
    let b = decoded_bulk(&mut c);
    for (i, expected) in [b"abc".as_slice(), b"xy".as_slice()].iter().enumerate() {
        let src = &s.records[i].source;
        assert_eq!(&b[src.start_offset..src.end_offset], *expected);
    }
    let m = get(&mut c, "/RSeStorage/Mone");
    let m_info = rse::meta(&m, &Limits::default()).unwrap();
    let expanded = rse::inflate(&m[m_info.compressed_offset..], 65536)
        .unwrap()
        .0;
    for (entry, expected) in s.meta.as_ref().unwrap().types.iter().zip([TYPE_A, TYPE_B]) {
        assert_eq!(
            &expanded[entry.source.start_offset..entry.source.end_offset],
            expected
        );
    }
}
#[test]
fn old_majors_and_unknown_kinds_remain_inventory_only() {
    for (major, kind) in [
        (22, "DlSheetDlSegmentType"),
        (25, "DlSheetDlSegmentType"),
        (31, "UnqualifiedType"),
    ] {
        let data = mutate(&fixture(), |c| {
            put(
                c,
                "/RSeStorage/RSeSegInfo",
                &registry(&[("DLSheet999DLSegment", major, [1; 16], kind)]),
            )
        });
        let before = crate::inspect(&data, "renamed.any", &Limits::default()).unwrap();
        let d = read(&data);
        assert!(has(&d, "drawing.unsupported_profile"));
        assert!(d.segments[0].records.is_empty());
        assert_eq!(d.usage.expanded_bytes, 0);
        assert_eq!(
            serde_json::to_value(d.metadata).unwrap(),
            serde_json::to_value(before.summary.document).unwrap()
        );
    }
}

#[test]
fn major23_requires_its_own_envelope_and_zlib_codec() {
    let data = mutate(&fixture(), |c| {
        put(
            c,
            "/RSeStorage/RSeSegInfo",
            &registry(&[("DLSheet999DLSegment", 23, [1; 16], "DlSheetDlSegmentType")]),
        );
        let mut b = profile::BULK_HEADER.to_vec();
        b[17] = 1;
        b.extend(zlib(&body()));
        put(c, "/RSeStorage/Bone", &b);
    });
    let doc = read(&data);
    assert_eq!(doc.status, "framed_subset");
    assert_eq!(doc.segments[0].records.len(), 2);
    assert_eq!(doc.segments[0].bulk.as_ref().unwrap().codec, "zlib");
    assert_eq!(doc.segments[0].profile, Some(profile::name(23)));
    for mutation in 0..4 {
        let bad = mutate(&data, |c| {
            let mut b = get(c, "/RSeStorage/Bone");
            match mutation {
                0 => b[17] = 2,
                1 => {
                    b.truncate(18);
                    b.extend(zstd(&body()));
                }
                2 => {
                    b.pop();
                }
                3 => b.extend(zlib(&body())),
                _ => unreachable!(),
            }
            put(c, "/RSeStorage/Bone", &b);
        });
        let rejected = read(&bad);
        assert!(
            rejected.segments[0].records.is_empty(),
            "mutation {mutation}"
        );
        assert!(has(&rejected, "drawing.bulk_framing_unavailable"));
    }
    let mut wire = profile::BULK_HEADER.to_vec();
    wire[17] = 1;
    wire.extend(zlib(&body()));
    assert!(profile::bulk_for_major(&wire, 31).is_err());
    for n in 0..20 {
        assert!(profile::bulk_for_major(&wire[..n], 23).is_err());
    }
}

#[test]
fn major23_list_variants_and_annotation_layouts_are_exact() {
    let parse = |major: u8, kind, ty, b: &[u8], work: &mut usize| {
        fields::decode(
            kind,
            major.into(),
            ty,
            0,
            b,
            SourceSpan::stream("test", "/B", 0, b.len()),
            work,
        )
    };
    for tag in [0x30000002u32, 0x30000003] {
        let mut b = vec![0; 26];
        word(&mut b, tag);
        word(&mut b, 1);
        if tag == 0x30000002 {
            word(&mut b, 1);
        }
        word(&mut b, 0);
        word(&mut b, 0x80000003);
        b.push(0);
        assert!(parse(23, "DlSheetDlSegmentType", GROUP_TYPE, &b, &mut 100)
            .unwrap()
            .is_some());
        assert!(parse(31, "DlSheetDlSegmentType", GROUP_TYPE, &b, &mut 100).is_err());
        assert!(parse(23, "DlSheetDlSegmentType", GROUP_TYPE, &b, &mut 0).is_err());
        for n in 0..b.len() {
            assert!(parse(23, "DlSheetDlSegmentType", GROUP_TYPE, &b[..n], &mut 100).is_err());
        }
        b.push(0);
        assert!(parse(23, "DlSheetDlSegmentType", GROUP_TYPE, &b, &mut 100).is_err());
    }
    let mut balloon = vec![0; 15];
    for n in [0x30000003, 0, 0x80000005, 9] {
        word(&mut balloon, n);
    }
    balloon.push(1);
    word(&mut balloon, 9);
    let ty = "025e3388-4cbb-8851-7d1c-b0876dcb2a07";
    assert_eq!(
        parse(23, "DlSheetSmSegmentType", ty, &balloon, &mut 100)
            .unwrap()
            .unwrap()
            .proposed_role,
        "sheet_local_display_candidate"
    );
    assert!(parse(31, "DlSheetSmSegmentType", ty, &balloon, &mut 100)
        .unwrap()
        .is_none());
    for n in 0..balloon.len() {
        assert!(parse(23, "DlSheetSmSegmentType", ty, &balloon[..n], &mut 100).is_err());
    }
}
#[test]
fn duplicate_registry_or_metadata_ids_have_no_scan_order_winner() {
    for registry_duplicate in [true, false] {
        let data = mutate(&fixture(), |c| {
            if registry_duplicate {
                put(
                    c,
                    "/RSeStorage/RSeSegInfo",
                    &registry(&[
                        ("DLSheet999DLSegment", 31, [1; 16], "DlSheetDlSegmentType"),
                        ("Other", 31, [1; 16], "DlSheetDlSegmentType"),
                    ]),
                );
            } else {
                let m = get(c, "/RSeStorage/Mone");
                put(c, "/RSeStorage/Mtwo", &m);
            }
        });
        let d = read(&data);
        assert!(has(&d, "drawing.ambiguous_owner"));
        assert!(d.segments.iter().all(|s| s.records.is_empty()));
        assert_eq!(d.usage.expanded_bytes, 0);
    }
}
#[test]
fn names_and_unidentifiable_document_meta_cannot_establish_owner() {
    for unknown in [true, false] {
        let data = mutate(&fixture(), |c| {
            if unknown {
                put(c, "/RSeStorage/Munknown", b"unqualified meta");
            } else {
                put(
                    c,
                    "/RSeStorage/Mone",
                    &metadata("Wrong", [1; 16], &[TYPE_A, TYPE_B]),
                );
            }
        });
        let d = read(&data);
        assert!(has(&d, "drawing.ambiguous_owner"));
        assert!(d.segments[0].records.is_empty());
    }
}
#[test]
fn template_namespace_is_opaque_and_never_supplies_a_missing_owner() {
    let data = mutate(&fixture(), |c| {
        c.create_storage_all("/RSeStorage/Templates").unwrap();
        let m = get(c, "/RSeStorage/Mone");
        put(c, "/RSeStorage/Templates/Mone", &m);
        put(c, "/RSeStorage/Templates/Bone", b"unsupported");
        put(
            c,
            "/RSeStorage/Templates/Munknown",
            b"unqualified template header",
        );
    });
    let d = read(&data);
    assert_eq!(d.status, "framed_subset");
    assert!(d
        .identities
        .iter()
        .filter(|m| m.scope == "template")
        .all(|m| m.id.is_none() && m.status == "opaque_namespace"));
    assert!(d.usage.encoded_bytes > read(&fixture()).usage.encoded_bytes);
    assert!(d
        .unclaimed_streams
        .iter()
        .any(|r| r.reason == "template_not_admitted"));
    let missing = mutate(&data, |c| c.remove_stream("/RSeStorage/Mone").unwrap());
    assert!(has(&read(&missing), "drawing.missing_meta"));
}
#[test]
fn multiple_databases_missing_bulk_and_orphans_remain_explicit() {
    let data = mutate(&fixture(), |c| {
        let b = get(c, "/RSeStorage/V1/RSeDb");
        c.create_storage_all("/RSeStorage/V2").unwrap();
        put(c, "/RSeStorage/V2/RSeDb", &b);
    });
    let d = read(&data);
    assert!(has(&d, "drawing.ambiguous_owner"));
    assert!(d.segments.is_empty());
    assert_eq!(d.metadata.databases.len(), 2);
    let data = mutate(&fixture(), |c| c.remove_stream("/RSeStorage/Bone").unwrap());
    assert!(has(&read(&data), "drawing.missing_bulk"));
    let data = mutate(&fixture(), |c| put(c, "/RSeStorage/Bdecoy", b"ACIS fake"));
    let d = read(&data);
    assert_eq!(d.status, "framed_subset");
    assert!(d
        .unclaimed_streams
        .iter()
        .any(|r| r.source.stream == "/RSeStorage/Bdecoy"));
}
#[test]
fn invalid_type_and_cut_record_trailers_admit_no_partial_record_list() {
    for mode in 0..4 {
        let data = mutate(&fixture(), |c| {
            let mut b = decoded_bulk(c);
            match mode {
                0 => b[0] = 2,
                1 => b[7..11].copy_from_slice(&99u32.to_le_bytes()),
                2 => b.truncate(14),
                _ => {
                    let end = b.len() - "opaque tail".len() - 4;
                    b.truncate(end);
                }
            };
            put(c, "/RSeStorage/Bone", &bulk(&b));
        });
        let d = read(&data);
        assert!(has(&d, "drawing.bulk_framing_unavailable"));
        assert!(d.segments[0].records.is_empty());
        assert!(d.segments[0].bulk.is_none());
        assert_eq!(
            d.segments[0].diagnostics[0].source.as_ref().unwrap().stream,
            "/RSeStorage/Bone"
        );
    }
}
#[test]
fn envelope_unknown_codec_extra_frame_and_meta_variant_fail_closed() {
    for mode in 0..4 {
        let data = mutate(&fixture(), |c| {
            if mode == 3 {
                put(
                    c,
                    "/RSeStorage/Mone",
                    &metadata("DLSheet999DLSegment", [1; 16], &[TYPE_A; 257]),
                );
                return;
            }
            let mut b = get(c, "/RSeStorage/Bone");
            match mode {
                0 => b[0] ^= 1,
                1 => {
                    b.truncate(18);
                    b.extend(zlib(&body()));
                }
                _ => b.extend(zstd(b"extra")),
            };
            put(c, "/RSeStorage/Bone", &b);
        });
        let d = read(&data);
        assert!(d.segments[0].records.is_empty());
        assert_eq!(d.segments[0].status, "unavailable");
        assert!(!d.segments[0].diagnostics.is_empty());
    }
}
#[test]
fn failures_spend_aggregate_inflate_and_work_budgets() {
    let data = mutate(&fixture(), |c| {
        put(
            c,
            "/RSeStorage/RSeSegInfo",
            &registry(&[
                ("DLSheet999DLSegment", 31, [1; 16], "DlSheetDlSegmentType"),
                ("Other", 31, [2; 16], "DlSheetDlSegmentType"),
            ]),
        );
        put(
            c,
            "/RSeStorage/Mtwo",
            &metadata("Other", [2; 16], &[TYPE_A, TYPE_B]),
        );
        put(c, "/RSeStorage/Btwo", &bulk(&body()));
    });
    let cap = read(&fixture()).usage.expanded_bytes + 10;
    let limits = Limits {
        max_total_inflated_bytes: cap,
        ..Default::default()
    };
    let d = inspect(&data, "test", &limits).unwrap();
    assert_eq!(d.usage.expanded_bytes, cap);
    assert_eq!(d.segments[0].records.len(), 2);
    assert!(d.segments[1].records.is_empty());
    assert_eq!(d.status, "partial");
    let limits = Limits {
        max_records: 6,
        ..Default::default()
    };
    let d = inspect(&data, "test", &limits).unwrap();
    assert_eq!(d.usage.work_items, 6);
    assert!(d.segments.iter().all(|s| s.records.is_empty()));
    let mut remaining = 1000;
    let mut compressed = zlib(&[7; 100]);
    compressed.pop();
    assert!(rse::inflate_budgeted(&compressed, 1000, &mut remaining).is_err());
    assert_eq!(remaining, 900);
}
#[test]
fn all_template_bytes_and_oversized_members_obey_limits() {
    let data = mutate(&fixture(), |c| {
        c.create_storage_all("/RSeStorage/Templates").unwrap();
        put(c, "/RSeStorage/Templates/Opaque", &[0; 4096]);
    });
    let limits = Limits {
        max_stream_bytes: 1024,
        ..Default::default()
    };
    let d = inspect(&data, "test", &limits).unwrap();
    assert!(has(&d, "drawing.inventory_unavailable"));
    assert_eq!(d.usage.expanded_bytes, 0);
    let data = mutate(&fixture(), |c| {
        let mut b = body();
        b.extend([0; 4096]);
        put(c, "/RSeStorage/Bone", &bulk(&b));
    });
    let limits = Limits {
        max_inflated_bytes: 1024,
        ..Default::default()
    };
    let d = inspect(&data, "test", &limits).unwrap();
    assert!(has(&d, "drawing.bulk_framing_unavailable"));
    assert!(d.segments[0].records.is_empty());
}
#[test]
fn zero_budgets_invalid_input_and_document_kind_are_not_bypassed() {
    assert!(inspect(b"fake", "test", &Limits::default()).is_err());
    assert!(inspect(&fixture(), "", &Limits::default()).is_err());
    for limits in [
        Limits {
            max_records: 0,
            ..Default::default()
        },
        Limits {
            max_total_inflated_bytes: 0,
            ..Default::default()
        },
    ] {
        let d = inspect(&fixture(), "test", &limits).unwrap();
        assert!(d.segments.iter().all(|s| s.records.is_empty()));
    }
    let data = mutate(&fixture(), |c| {
        c.set_storage_clsid("/", "4d29b490-49b2-11d0-93c3-7e0706000000".parse().unwrap())
            .unwrap()
    });
    let d = read(&data);
    assert!(has(&d, "drawing.document_kind"));
    assert!(d.segments.is_empty());
}
#[test]
fn named_reference_repetitions_spend_work_without_following_cycles() {
    let meta = rse::meta(
        &metadata("test", [1; 16], &[TYPE_A, TYPE_B]),
        &Limits::default(),
    )
    .unwrap();
    let mut work = 4;
    assert!(rse::record_table(&body(), &meta, 31, &mut work, true).is_err());
    assert_eq!(work, 0);
    let mut work = 5;
    let table = rse::record_table(&body(), &meta, 31, &mut work, true).unwrap();
    assert_eq!(work, 0);
    assert_eq!(table.records[0].references.len(), 2);
}

fn dc_metadata(types: usize, entries: &[u8], count: usize) -> Vec<u8> {
    let name = "DlDocDCSegment";
    let sample = metadata(name, [1; 16], &[TYPE_A]);
    let offset = rse::meta(&sample, &Limits::default())
        .unwrap()
        .compressed_offset;
    let encoded = metadata(name, [1; 16], &vec![TYPE_A; types]);
    let mut body = rse::inflate(&encoded[offset..], 1024 * 1024).unwrap().0;
    let payload = body.len() - 104;
    body[payload - 4..payload].copy_from_slice(&(count as u32).to_le_bytes());
    body.splice(payload..payload, entries.iter().copied());
    let next = payload + entries.len();
    body[next..next + 4].copy_from_slice(&(4 + entries.len() as u32).to_le_bytes());
    let mut out = encoded[..offset].to_vec();
    out.extend(zstd(&body));
    out
}

#[test]
fn drawing_dc_meta_keeps_extra_descriptors_and_requires_exact_section_chain() {
    let limits = Limits::default();
    let entry = |tag, size| {
        let mut bytes = vec![1, 0, tag];
        bytes.extend(vec![0; size]);
        bytes
    };
    let mixed = [entry(3, 8), entry(2, 16)].concat();
    let bytes = dc_metadata(303, &mixed, 2);
    let parse = |bytes: &[u8], layout| {
        rse::meta_layout_budgeted(
            bytes,
            &limits,
            &mut { limits.max_inflated_bytes },
            &mut { limits.max_records },
            layout,
        )
    };
    let meta = parse(&bytes, rse::MetaLayout::DRAWING_DOC_DC).unwrap();
    assert_eq!(meta.types.len(), 303);
    // Unused descriptors above 255 are retained; they do not widen the selector.
    let table = rse::record_table(&body(), &meta, 31, &mut { 1000 }, true).unwrap();
    assert_eq!(table.records[0].kind, TYPE_A);
    assert!(parse(&bytes, rse::MetaLayout::STANDARD).is_err());
    assert!(parse(&bytes, rse::MetaLayout::DRAWING_SHEET_DC).is_err());
    assert!(parse(
        &dc_metadata(4097, &mixed, 2),
        rse::MetaLayout::DRAWING_DOC_DC
    )
    .is_err());
    for entries in [
        entry(3, 8),
        entry(1, 16),
        entry(2, 16),
        [entry(3, 8), entry(3, 8), entry(2, 16)].concat(),
    ] {
        let count = if entries.len() == 41 { 3 } else { 1 };
        assert!(parse(
            &dc_metadata(75, &entries, count),
            rse::MetaLayout::DRAWING_SHEET_DC
        )
        .is_ok());
    }
    // Reject arbitrary 15-byte entries, unknown tags, count mismatches, truncation
    // and extra bytes; a valid backwards span alone must not admit a payload.
    for (entries, count) in [
        (vec![0; 30], 2),
        (entry(4, 8), 1),
        (entry(3, 7), 1),
        (entry(3, 9), 1),
        (entry(1, 15), 1),
        (mixed.clone(), 1),
        (mixed.clone(), 3),
        (mixed.clone(), limits.max_records + 1),
    ] {
        assert!(parse(
            &dc_metadata(75, &entries, count),
            rse::MetaLayout::DRAWING_SHEET_DC
        )
        .is_err());
    }
    let mut broken = bytes.clone();
    broken.pop();
    assert!(parse(&broken, rse::MetaLayout::DRAWING_DOC_DC).is_err());
}

fn observed(
    kind: &str,
    type_id: &str,
    data: &[u8],
    work: &mut usize,
) -> crate::Result<Option<PayloadObservation>> {
    let mut source = SourceSpan::stream("synthetic", "/RSeStorage/Btest", 100, 100 + data.len());
    source.byte_domain = "inflated_stream";
    super::fields::decode(kind, 31.into(), type_id, 17, data, source, work)
}
const TEXT_TYPE: &str = "a79eacd5-11d1-c281-6000-a38ab46bceb0";
const POINT_TYPE: &str = "a79eaccb-11d1-c281-6000-a38ab46bceb0";
const GROUP_TYPE: &str = "a79eaccf-11d1-c281-6000-a38ab46bceb0";
const TRIANGLES_TYPE: &str = "a79eacd2-11d1-c281-6000-a38ab46bceb0";

#[test]
fn saved_triangle_batches_require_exact_profile_indices_and_auxiliary_layout() {
    for (major, vertex_flags, index_flags) in [(26, 0x102, 0), (28, 0x102, 8), (29, 0x102, 8)] {
        for count in [3, 6] {
            let mut b = vec![0; 26];
            for n in [0x30000002, count, count, vertex_flags] {
                word(&mut b, n);
            }
            for _ in 0..count / 3 {
                for v in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
                    b.extend(v.to_le_bytes());
                }
            }
            let list = b.len();
            for n in [0x30000002, count, count, index_flags] {
                word(&mut b, n);
            }
            for n in 0..count {
                word(&mut b, n);
            }
            let suffix = b.len();
            for n in [0x30000002, 0, 0x30000002, 0, 0x105, 0x30000002, 0, 0, 0] {
                word(&mut b, n);
            }
            let parse = |major, bytes: &[u8], work: &mut usize| {
                observed_major(major, "DlSheetSmSegmentType", TRIANGLES_TYPE, bytes, work)
            };
            let o = parse(major, &b, &mut 100).unwrap().unwrap();
            assert_eq!(o.proposed_role, "stored_triangles_candidate");
            assert!(
                matches!(&o.fields.last().unwrap().value, FieldValue::U32(v) if v == &(0..count).collect::<Vec<_>>())
            );
            for n in 0..b.len() {
                assert!(parse(major, &b[..n], &mut 100).is_err());
            }
            for (offset, value) in [
                (30, 9),
                (34, 2),
                (38, 0x103),
                (42, f32::INFINITY.to_bits()),
                (list + 4, count + 1),
                (list + 8, 2),
                (list + 12, 7),
                (list + 16, count),
                (suffix + 4, 1),
            ] {
                let mut bad = b.clone();
                bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                assert!(
                    parse(major, &bad, &mut 100).is_err(),
                    "major {major} offset {offset}"
                );
            }
            for other in [26, 28, 29] {
                if (other == 26) != (major == 26) {
                    assert!(parse(other, &b, &mut 100).is_err());
                }
            }
            for other in [23, 24, 31] {
                assert!(parse(other, &b, &mut 100).unwrap().is_none());
            }
            assert!(parse(major, &b, &mut 0).is_err());
            b.push(0);
            assert!(parse(major, &b, &mut 100).is_err());
        }
    }
}
fn observed_major(
    major: u8,
    kind: &str,
    type_id: &str,
    data: &[u8],
    work: &mut usize,
) -> crate::Result<Option<PayloadObservation>> {
    let mut source = SourceSpan::stream("synthetic", "/RSeStorage/Btest", 100, 100 + data.len());
    source.byte_domain = "inflated_stream";
    super::fields::decode(kind, major.into(), type_id, 17, data, source, work)
}

#[test]
fn added_profiles_require_their_exact_envelope_and_codec() {
    for major in [24, 29, 28, 26] {
        let mut b = profile::BULK_HEADER.to_vec();
        b[17] = 1;
        b.extend(zlib(b"synthetic record bytes"));
        assert!(profile::bulk_for_major(&b, major).is_ok());
        assert!(profile::bulk_for_major(&b, 31).is_err());
        for unknown in [21, 25, 27, 30, 32] {
            assert!(profile::bulk_for_major(&b, unknown).is_err());
            assert!(!profile::admits("DlSheetDlSegmentType", unknown));
        }
        for n in 0..20 {
            assert!(profile::bulk_for_major(&b[..n], major).is_err());
        }
        let mut wrong_codec = b[..18].to_vec();
        wrong_codec.extend(zstd(b"synthetic record bytes"));
        assert!(profile::bulk_for_major(&wrong_codec, major).is_err());
        b[17] = 2;
        assert!(profile::bulk_for_major(&b, major).is_err());
    }
}

#[test]
fn reference_list_tag_and_flags_are_independent_version_choices() {
    for major in [23, 24, 26, 28, 29, 31] {
        for tag in [0x30000002, 0x30000003] {
            for flags in [0, 0x10] {
                let mut b = vec![0; 26];
                word(&mut b, tag);
                word(&mut b, 1);
                if tag == 0x30000002 {
                    word(&mut b, 1);
                }
                word(&mut b, flags);
                word(&mut b, 0x80000012);
                b.push(0);
                let accepted = flags == if major <= 26 { 0 } else { 0x10 }
                    && (tag == 0x30000002 || matches!(major, 23 | 24 | 26 | 28));
                let parsed =
                    observed_major(major, "DlSheetDlSegmentType", GROUP_TYPE, &b, &mut 100);
                assert_eq!(
                    parsed.is_ok(),
                    accepted,
                    "major {major}, tag {tag:x}, flags {flags}"
                );
                if accepted {
                    assert!(parsed.unwrap().is_some());
                    for end in 0..b.len() {
                        assert!(observed_major(
                            major,
                            "DlSheetDlSegmentType",
                            GROUP_TYPE,
                            &b[..end],
                            &mut 100
                        )
                        .is_err());
                    }
                    assert!(
                        observed_major(major, "DlSheetDlSegmentType", GROUP_TYPE, &b, &mut 0)
                            .is_err()
                    );
                }
            }
        }
    }
}

#[test]
fn sheet_links_require_profile_prefix_and_complete_suffix() {
    for (major, prefix, suffix) in [(24, 30, 4), (29, 34, 8), (28, 34, 8), (26, 30, 8)] {
        let mut b = vec![0; prefix];
        for n in [0x30000002, 0] {
            word(&mut b, n);
        }
        b.extend([0; 11]);
        for name in [
            "DLSheet1DCSegment",
            "DLSheet1DLSegment",
            "DLSheet1SMSegment",
        ] {
            wide(&mut b, name);
        }
        b.extend([0; 28]);
        for n in [0x30000002, 0] {
            word(&mut b, n);
        }
        b.extend([0; 8]);
        for n in [0x30000002, 0, 0x30000006, 0] {
            word(&mut b, n);
        }
        b.extend([0; 8]);
        for n in [0x30000002, 0] {
            word(&mut b, n);
        }
        b.extend([0; 92]);
        wide(&mut b, "Synthetic sheet");
        b.extend(vec![0; suffix]);
        let parse = |data: &[u8]| {
            observed_major(
                major,
                "DlDocDcSegmentType",
                "a200fb76-11d1-6107-0008-70bdec18db09",
                data,
                &mut 1000,
            )
        };
        let o = parse(&b).unwrap().unwrap();
        assert!(o.fields.iter().any(|f| f.name == "name"
            && matches!(&f.value, FieldValue::Utf16(s) if s == "Synthetic sheet")));
        for end in 0..b.len() {
            assert!(parse(&b[..end]).is_err());
        }
        if major == 26 {
            let last = b.len() - 1;
            b[last] = 1;
            assert!(parse(&b).is_err());
            b[last] = 0;
        }
        b.push(0);
        assert!(parse(&b).is_err());
    }
}

#[test]
fn intermediate_view_layouts_have_two_lists_and_a_suffix_guid() {
    for major in [26, 28, 29] {
        let mut b = vec![0; 15];
        for n in [0x30000002, 0, 0x80000001] {
            word(&mut b, n);
        }
        b.push(1);
        word(&mut b, 0x80000002);
        b.extend(0x8421u16.to_le_bytes());
        b.extend(0x7bdeu16.to_le_bytes());
        b.push(1);
        for n in [0x80000002, 0, 0x30000002, 0, 0x30000002, 0] {
            word(&mut b, n);
        }
        wide(&mut b, "Synthetic view");
        for n in [0x80000003, 0] {
            word(&mut b, n);
        }
        b.extend([0; 48 + 16]);
        let parse = |major, data: &[u8]| {
            observed_major(
                major,
                "DlSheetSmSegmentType",
                "8a6d1381-11d1-6b56-6000-38bd861c3cb0",
                data,
                &mut 1000,
            )
        };
        assert!(parse(major, &b).unwrap().is_some());
        assert!(parse(23, &b).is_err());
        assert!(parse(31, &b).is_err());
        assert!(parse(24, &b).unwrap().is_none());
        for end in 0..b.len() {
            assert!(parse(major, &b[..end]).is_err());
        }
    }
}
fn text_payload() -> Vec<u8> {
    let mut b = vec![0; 26];
    wide(&mut b, "図面\0𝄞");
    for n in [-2.5f64, 4., 9., 0., -1., 0.] {
        b.extend(n.to_le_bytes());
    }
    b.extend(9u16.to_le_bytes());
    word(&mut b, 68);
    b.push(0);
    b
}
#[test]
fn typed_text_preserves_unicode_nul_z_and_unqualified_source_fields() {
    let b = text_payload();
    let o = observed("DlSheetDlSegmentType", TEXT_TYPE, &b, &mut 100)
        .unwrap()
        .unwrap();
    assert_eq!(o.status, "unqualified");
    assert_eq!(o.record_ordinal, 17);
    assert_eq!(o.fields[4].source.start_offset, 126);
    assert_eq!(o.fields[4].source.end_offset, 140);
    assert!(matches!(&o.fields[4].value, FieldValue::Utf16(s) if s=="図面\0𝄞"));
    assert!(matches!(&o.fields[5].value, FieldValue::F64(v) if v==&[-2.5,4.,9.,0.,-1.,0.]));
    let sm = observed("DlSheetSmSegmentType", TEXT_TYPE, &b, &mut 100)
        .unwrap()
        .unwrap();
    assert_eq!(sm.proposed_role, "stored_text_candidate");
    assert_eq!(
        serde_json::to_value(&sm.fields).unwrap(),
        serde_json::to_value(&o.fields).unwrap()
    );
    // Same type in DocDC can be a definition; it must not become displayed text.
    assert!(observed("DlDocDcSegmentType", TEXT_TYPE, &b, &mut 100)
        .unwrap()
        .is_none());
    assert!(observed(
        "DlSheetDlSegmentType",
        "a79eacd5-11d1-c281-6000-a38ab46bceb1",
        &b,
        &mut 100
    )
    .unwrap()
    .is_none());
    for i in 0..b.len() {
        assert!(observed("DlSheetDlSegmentType", TEXT_TYPE, &b[..i], &mut 100).is_err());
        assert!(observed("DlSheetSmSegmentType", TEXT_TYPE, &b[..i], &mut 100).is_err());
    }
    let mut invalid = b.clone();
    invalid[30..32].copy_from_slice(&0xd800u16.to_le_bytes());
    assert!(observed("DlSheetDlSegmentType", TEXT_TYPE, &invalid, &mut 100).is_err());
    let mut invalid = b.clone();
    invalid[40..48].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(observed("DlSheetDlSegmentType", TEXT_TYPE, &invalid, &mut 100).is_err());
    let mut extra = b.clone();
    extra.push(0);
    assert!(observed("DlSheetDlSegmentType", TEXT_TYPE, &extra, &mut 100).is_err());
    let mut work = 1;
    assert!(observed("DlSheetDlSegmentType", TEXT_TYPE, &b, &mut work).is_err());
    assert_eq!(work, 0);
}
#[test]
fn border_placement_requires_complete_suffix_and_observed_major() {
    let mut b = vec![0; 15];
    for n in [0x30000003, 0, 3] {
        word(&mut b, n);
    }
    b.push(1);
    word(&mut b, 4);
    b.extend(0x8421u16.to_le_bytes());
    b.extend(0x7bdeu16.to_le_bytes());
    b.push(1);
    word(&mut b, 4);
    word(&mut b, 4);
    let parse = |major, bytes: &[u8]| {
        observed_major(
            major,
            "DlSheetSmSegmentType",
            "62a8e6a8-11d1-ad4b-6000-108a806bceb0",
            bytes,
            &mut 100,
        )
    };
    for major in [24, 26, 28] {
        assert_eq!(
            parse(major, &b).unwrap().unwrap().proposed_role,
            "sheet_placement_candidate"
        );
        for n in 0..b.len() {
            assert!(parse(major, &b[..n]).is_err());
        }
        let mut bad = b.clone();
        bad[36] = 2;
        assert!(parse(major, &bad).is_err());
        bad = b.clone();
        bad.push(0);
        assert!(parse(major, &bad).is_err());
    }
    for major in [23, 29, 31] {
        assert!(parse(major, &b).unwrap().is_none());
    }
}

#[test]
fn point_encoding_resolves_the_owning_meta_type_table() {
    let point = [
        0xc1, 0x10, 0x72, 0xf2, 0xd2, 0x11, 0xa2, 0xcd, 0xa0, 0, 0x56, 0xb6, 0xfc, 0xdb, 0xc7, 0xc9,
    ];
    let mut b = vec![0; 26];
    for n in [0x30000002, 2, 2, 0x103] {
        word(&mut b, n);
    }
    for n in [1f32, 2., 0., 3., 4., 0.] {
        b.extend(n.to_le_bytes());
    }
    b.push(0);
    let parse = |b: &[u8], types: &[[u8; 16]]| {
        fields::decode(
            "DlSheetDlSegmentType",
            fields::Context::new(28, types),
            POINT_TYPE,
            0,
            b,
            SourceSpan::stream("synthetic", "/B", 0, b.len()),
            &mut 100,
        )
    };
    let types = [[0; 16], [0; 16], [0; 16], point];
    assert!(parse(&b, &types).unwrap().is_some());
    assert!(parse(&b, &types[..3]).is_err());
    assert!(parse(&b, &[[0; 16], [0; 16], point, [0; 16]]).is_err());
    for selector in [3u32, 0x203, 0x10003, 0x1ff] {
        let mut invalid = b.clone();
        invalid[38..42].copy_from_slice(&selector.to_le_bytes());
        assert!(parse(&invalid, &types).is_err());
    }
    b[38..42].copy_from_slice(&0x102u32.to_le_bytes());
    assert!(parse(&b, &[[0; 16], [0; 16], point]).unwrap().is_some());
}

#[test]
fn typed_points_reject_unknown_encoding_and_nonfinite_without_projecting_z() {
    let mut b = vec![0; 26];
    for n in [0x30000002, 2, 3, 0x102] {
        word(&mut b, n);
    }
    for n in [1f32, -2., 3., 4., 5., 6.] {
        b.extend(n.to_le_bytes());
    }
    b.push(0);
    let o = observed("DlSheetDlSegmentType", POINT_TYPE, &b, &mut 100)
        .unwrap()
        .unwrap();
    assert!(matches!(&o.fields[4].value,FieldValue::F32(v) if v==&[1.,-2.,3.,4.,5.,6.]));
    assert_eq!(o.fields[4].source.start_offset, 142);
    for i in 0..b.len() {
        assert!(observed("DlSheetDlSegmentType", POINT_TYPE, &b[..i], &mut 100).is_err());
    }
    for (offset, value) in [
        (30, 65537u32),
        (34, 1),
        (38, 0x103),
        (42, f32::INFINITY.to_bits()),
    ] {
        let mut invalid = b.clone();
        invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(observed("DlSheetDlSegmentType", POINT_TYPE, &invalid, &mut 100).is_err());
    }
}
#[test]
fn display_group_keeps_repeated_self_references_and_both_compact_forms_raw() {
    for prefix in [false, true] {
        let mut b = vec![0; 26];
        for n in [0x30000002, 2, 2, 0x10, 0x80000012, 0x80000012] {
            word(&mut b, n);
        }
        b.push(1);
        if prefix {
            word(&mut b, 0x203);
        }
        b.extend(0x8421u16.to_le_bytes());
        b.extend(0x7b56u16.to_le_bytes());
        b.extend(5f64.to_le_bytes());
        b.extend((-7f64).to_le_bytes());
        let o = observed("DlSheetDlSegmentType", GROUP_TYPE, &b, &mut 100)
            .unwrap()
            .unwrap();
        assert_eq!(o.fields.len(), 7);
        assert!(matches!(&o.fields[4].value,FieldValue::U32(v) if v==&[0x80000012,0x80000012]));
        assert!(
            matches!(&o.fields[6].value,FieldValue::CompactTransform {prefixed,values,..}
            if *prefixed==prefix && values[3]==5. && values[7]==-7.)
        );
        for i in 0..b.len() {
            assert!(observed("DlSheetDlSegmentType", GROUP_TYPE, &b[..i], &mut 100).is_err());
        }
    }
}
#[test]
fn sheet_name_and_links_use_counted_fields_not_file_offsets_or_segment_name_scans() {
    let mut b = vec![0; 34];
    wide(&mut b, "別シート");
    b.extend([0; 134]);
    let o = observed(
        "DlSheetDcSegmentType",
        "d37c90cd-11d0-fa16-6000-0dbd861c3cb0",
        &b,
        &mut 100,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(&o.fields[0].value,FieldValue::Utf16(s) if s=="別シート"));
    let mut b = vec![0; 34];
    word(&mut b, 0x30000002);
    word(&mut b, 0);
    b.extend([0; 11]);
    for s in [
        "DLSheet999DCSegment",
        "DLSheet999DLSegment",
        "DLSheet999SMSegment",
    ] {
        wide(&mut b, s);
    }
    b.extend([0; 28]);
    word(&mut b, 0x30000002);
    word(&mut b, 0);
    b.extend([0; 8]);
    word(&mut b, 0x30000002);
    word(&mut b, 0);
    word(&mut b, 0x30000006);
    word(&mut b, 0);
    b.extend([0; 8]);
    word(&mut b, 0x30000002);
    word(&mut b, 0);
    b.extend([0; 92]);
    wide(&mut b, "別シート");
    b.extend([0; 8]);
    let o = observed(
        "DlDocDcSegmentType",
        "a200fb76-11d1-6107-0008-70bdec18db09",
        &b,
        &mut 1000,
    )
    .unwrap()
    .unwrap();
    assert_eq!(o.fields.len(), 10);
    assert_eq!(o.fields[0].name, "header_flags");
    assert_eq!(o.fields[1].name, "object_id");
    assert!(matches!(&o.fields[4].value,FieldValue::Utf16(s) if s=="DLSheet999DLSegment"));
    assert!(matches!(&o.fields[9].value,FieldValue::Utf16(s) if s=="別シート"));
    for index in [6, 7, 8] {
        let list_start = o.fields[index].source.start_offset - 100 - 8;
        for refs in [vec![0x80000002], vec![0x80000003, 0x80000005]] {
            let mut list = vec![];
            for n in [0x30000002, refs.len() as u32, refs.len() as u32, 0x10] {
                word(&mut list, n);
            }
            for n in refs {
                word(&mut list, n);
            }
            let mut variable = b.clone();
            variable.splice(list_start..list_start + 8, list);
            let o = observed(
                "DlDocDcSegmentType",
                "a200fb76-11d1-6107-0008-70bdec18db09",
                &variable,
                &mut 1000,
            )
            .unwrap()
            .unwrap();
            assert!(matches!(&o.fields[9].value,FieldValue::Utf16(s) if s=="別シート"));
            for end in 0..variable.len() {
                assert!(observed(
                    "DlDocDcSegmentType",
                    "a200fb76-11d1-6107-0008-70bdec18db09",
                    &variable[..end],
                    &mut 1000
                )
                .is_err());
            }
        }
    }
}
#[test]
fn typed_fields_require_framed_unique_owners_and_fail_without_partial_observations() {
    let kind = [
        0xd5, 0xac, 0x9e, 0xa7, 0xd1, 0x11, 0x81, 0xc2, 0x60, 0, 0xa3, 0x8a, 0xb4, 0x6b, 0xce, 0xb0,
    ];
    let make = |payload: &[u8]| {
        mutate(&fixture(), |c| {
            let bytes = metadata("DLSheet999DLSegment", [1; 16], &[kind, TYPE_B]);
            let meta = rse::meta(&bytes, &Limits::default()).unwrap();
            let mut expanded = rse::inflate(&bytes[meta.compressed_offset..], 1024 * 1024)
                .unwrap()
                .0;
            expanded[meta.block_table_offset..meta.block_table_offset + 4]
                .copy_from_slice(&(0x80000000 | payload.len() as u32).to_le_bytes());
            let mut encoded = bytes[..meta.compressed_offset].to_vec();
            encoded.extend(zstd(&expanded));
            put(c, "/RSeStorage/Mone", &encoded);
            let mut b = vec![];
            word(&mut b, 0x100);
            b.extend(payload);
            word(&mut b, payload.len() as u32);
            b.push(0);
            word(&mut b, 1);
            b.extend(b"xy");
            word(&mut b, 2);
            b.push(0);
            word(&mut b, u32::MAX);
            put(c, "/RSeStorage/Bone", &bulk(&b));
        })
    };
    let good = make(&text_payload());
    let d = read(&good);
    assert_eq!(d.segments[0].observations.len(), 1);
    assert_eq!(d.sheet_count, None);
    let duplicated = mutate(&good, |c| {
        let m = get(c, "/RSeStorage/Mone");
        put(c, "/RSeStorage/Mduplicate", &m);
    });
    assert!(read(&duplicated).segments[0].observations.is_empty());
    let mut invalid = text_payload();
    invalid.push(0);
    let d = read(&make(&invalid));
    assert_eq!(d.segments[0].status, "framed");
    assert_eq!(d.segments[0].records.len(), 2);
    assert!(d.segments[0].observations.is_empty());
    assert!(has(&d, "drawing.fields_unavailable"));
}

#[test]
fn document_list_prefix_preserves_order_without_resolving_sheet_count() {
    let mut b = vec![0; 34];
    wide(&mut b, "Document");
    word(&mut b, 0x30000002);
    word(&mut b, 0);
    b.extend([0; 36]);
    for n in [0x30000002, 2, 2, 0x10, 0x80000013, 0x80000008] {
        word(&mut b, n);
    }
    b.extend(b"opaque remaining state");
    let o = observed(
        "DlDocDcSegmentType",
        "d37c90cb-11d0-fa16-6000-0dbd861c3cb0",
        &b,
        &mut 100,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(&o.fields[2].value,FieldValue::U32(v) if v==&[0x80000013,0x80000008]));
    assert_eq!(o.status, "unqualified");
}

#[test]
fn sheet_space_preserves_raw_extent_and_rejects_unknown_branch() {
    let mut b = vec![0; 15];
    word(&mut b, 0x30000002);
    word(&mut b, 0);
    word(&mut b, 1);
    b.push(1);
    word(&mut b, 2);
    word(&mut b, 0x203);
    b.extend(0x8421u16.to_le_bytes());
    b.extend(0x7bdeu16.to_le_bytes());
    b.push(1);
    word(&mut b, 2);
    for n in [-3.5f64, 7.5, 84., 59.4] {
        b.extend(n.to_le_bytes());
    }
    let kind = "DlSheetSmSegmentType";
    let typ = "f4a2f948-11d1-7bd2-0008-7abdec18db09";
    let o = observed(kind, typ, &b, &mut 100).unwrap().unwrap();
    assert!(matches!(&o.fields[4].value,FieldValue::F64(v) if v==&[-3.5,7.5,84.,59.4]));
    for i in 0..b.len() {
        assert!(observed(kind, typ, &b[..i], &mut 100).is_err());
    }
    b[40] = 2;
    assert!(observed(kind, typ, &b, &mut 100).is_err());
}

#[test]
fn sketch_and_local_note_placements_require_complete_known_layouts() {
    let mut sketch = vec![0; 15];
    word(&mut sketch, 0x30000002);
    word(&mut sketch, 0);
    word(&mut sketch, 1);
    sketch.push(1);
    word(&mut sketch, 2);
    sketch.extend(0x8421u16.to_le_bytes());
    sketch.extend(0x7bdeu16.to_le_bytes());
    sketch.push(1);
    word(&mut sketch, 0x80000007);
    let mut local = vec![0; 15];
    for value in [0x30000002, 0, 0x80000004, 2] {
        word(&mut local, value);
    }
    local.push(1);
    for (typ, bytes, branch) in [
        ("576520b3-11d1-a496-6000-1b8aeb49cdb0", sketch, 36),
        ("5eb510c2-11d2-7068-6000-f191790357b0", local, 31),
    ] {
        assert!(observed("DlSheetSmSegmentType", typ, &bytes, &mut 100)
            .unwrap()
            .is_some());
        for n in 0..bytes.len() {
            assert!(observed("DlSheetSmSegmentType", typ, &bytes[..n], &mut 100).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(observed("DlSheetSmSegmentType", typ, &extra, &mut 100).is_err());
        let mut unknown = bytes;
        unknown[branch] = 2;
        assert!(observed("DlSheetSmSegmentType", typ, &unknown, &mut 100).is_err());
    }
}

#[test]
fn style_and_image_wire_layouts_preserve_values_and_reject_incomplete_records() {
    let mut list = vec![];
    for n in [0x30000002, 1, 1, 0x10000000, 77] {
        word(&mut list, n);
    }
    let mut stroke = vec![];
    word(&mut stroke, 8);
    stroke.extend(0u16.to_le_bytes());
    stroke.extend(0.025f32.to_le_bytes());
    stroke.extend(1u16.to_le_bytes());
    stroke.extend(0u16.to_le_bytes());
    stroke.push(0);
    stroke.extend(2u16.to_le_bytes());
    stroke.extend(2u16.to_le_bytes());
    stroke.extend(0.4f64.to_le_bytes());
    stroke.extend((-0.1f64).to_le_bytes());
    for x in [-10000f32, 1., 0.] {
        stroke.extend(x.to_le_bytes());
    }
    word(&mut stroke, 0);
    word(&mut stroke, 1);
    let mut image = vec![0; 15];
    image.extend(2f64.to_le_bytes());
    image.extend(4f64.to_le_bytes());
    word(&mut image, 11);
    image.extend(0x8421u16.to_le_bytes());
    image.extend(0x7bdeu16.to_le_bytes());
    word(&mut image, 0xffffff);
    image.push(2);
    word(&mut image, 99);
    for (kind, typ, data) in [
        (
            "DlSheetDlSegmentType",
            "48eb8607-11d2-070c-6000-f99ac5361ab0",
            list,
        ),
        (
            "DlSheetDlSegmentType",
            "b32bf6a3-11d2-09f4-6000-f99ac5361ab0",
            vec![4, 0, 0, 0, 0],
        ),
        (
            "DlSheetDlSegmentType",
            "b32bf6ac-11d2-09f4-6000-f99ac5361ab0",
            stroke,
        ),
        (
            "DlSheetSmSegmentType",
            "5741c02f-4467-1e22-0ba3-53bd0da0bc81",
            image,
        ),
    ] {
        assert!(observed(kind, typ, &data, &mut 1000).unwrap().is_some());
        for n in 0..data.len() {
            assert!(observed(kind, typ, &data[..n], &mut 1000).is_err());
        }
        let mut extra = data.clone();
        extra.push(0);
        assert!(observed(kind, typ, &extra, &mut 1000).is_err());
        assert!(observed(kind, typ, &data, &mut 0).is_err());
    }
}

#[test]
fn annotation_local_wire_variants_are_exact_and_bounded() {
    for (kind, extra) in [
        ("5e4e86c7-11d0-fe3f-6000-0dbd351c3cb0", false),
        ("028c9254-11d1-e176-6000-2bb209e1b5b0", false),
        ("b86459e3-11d4-f88c-1000-cdab7dd247b5", true),
    ] {
        let mut b = vec![0; 15];
        for n in [0x30000002, 0, 0x80000004, 72] {
            word(&mut b, n);
        }
        b.push(1);
        if extra {
            word(&mut b, 72);
        }
        let o = observed("DlSheetSmSegmentType", kind, &b, &mut 100)
            .unwrap()
            .unwrap();
        assert_eq!(o.proposed_role, "sheet_local_display_candidate");
        for end in 0..b.len() {
            assert!(observed("DlSheetSmSegmentType", kind, &b[..end], &mut 100).is_err());
        }
        b[31] = 2;
        assert!(observed("DlSheetSmSegmentType", kind, &b, &mut 100).is_err());
        b[31] = 1;
        b.push(0);
        assert!(observed("DlSheetSmSegmentType", kind, &b, &mut 100).is_err());
    }
}
