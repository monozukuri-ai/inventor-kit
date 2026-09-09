use super::*;
use std::io::Write;
fn u32b(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_le_bytes());
}
fn text(out: &mut Vec<u8>, s: &str) {
    u32b(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}
fn wide(out: &mut Vec<u8>, s: &str) {
    let w = s.encode_utf16().collect::<Vec<_>>();
    u32b(out, w.len() as u32);
    for n in w {
        out.extend_from_slice(&n.to_le_bytes());
    }
}
fn compress(bytes: &[u8], zstd: bool) -> Vec<u8> {
    if zstd {
        zstd::stream::encode_all(bytes, 1).unwrap()
    } else {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(bytes).unwrap();
        e.finish().unwrap()
    }
}
fn sab() -> Vec<u8> {
    let mut b = b"ASM BinaryFile4".to_vec();
    for n in [22600, 0, 0, 0] {
        u32b(&mut b, n);
    }
    for s in ["synthetic", "ASM", "date"] {
        b.extend_from_slice(&[7, s.len() as u8]);
        b.extend_from_slice(s.as_bytes());
    }
    for n in [10.0_f64, 1e-6, 1e-10] {
        b.push(6);
        b.extend_from_slice(&n.to_le_bytes());
    }
    b.extend_from_slice(
        b"\x0d\x09asmheader\x0c\xff\xff\xff\xff\x04\xff\xff\xff\xff\x11\x0d\x0fEnd-of-ASM-data",
    );
    b
}
pub(crate) fn container(zstd: bool, duplicate_meta: bool, decoy: bool) -> Vec<u8> {
    let mut c = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    c.create_storage_all("/RSeStorage/V24").unwrap();
    let mut db = vec![0; 16];
    u32b(&mut db, 31);
    db.extend_from_slice(&[0; 32]);
    wide(&mut db, "");
    c.create_stream("/RSeStorage/V24/RSeDb")
        .unwrap()
        .write_all(&db)
        .unwrap();
    let mut registry = vec![];
    u32b(&mut registry, 1);
    wide(&mut registry, "PmBRepSegment");
    registry.extend_from_slice(&[1; 16]);
    registry.extend_from_slice(&[0; 16 + 4]);
    u32b(&mut registry, 0);
    registry.extend_from_slice(&[0; 24]);
    wide(&mut registry, "PmBrepSegmentType");
    registry.extend_from_slice(&[0; 8]);
    registry.extend_from_slice(&[0, 0, 25, 0, 0, 0, 0, 0]);
    registry.extend_from_slice(&[0; 4]);
    registry.extend_from_slice(&[0; 12]);
    c.create_stream("/RSeStorage/RSeSegInfo")
        .unwrap()
        .write_all(&registry)
        .unwrap();
    let mut carrier = vec![0; 14];
    carrier.extend(sab());
    u32b(&mut carrier, 0x8000_0001);
    carrier.push(1);
    u32b(&mut carrier, 0);
    carrier.push(0);
    u32b(&mut carrier, u32::MAX);
    u32b(&mut carrier, u32::MAX);
    let mut tables = vec![0; 14];
    u32b(&mut tables, 1);
    u32b(&mut tables, carrier.len() as u32 | 0x8000_0000);
    u32b(&mut tables, 8);
    for _ in 0..2 {
        u32b(&mut tables, 0);
        u32b(&mut tables, 4);
    }
    u32b(&mut tables, 1);
    tables.extend_from_slice(&[
        0x5c, 0x59, 0x45, 0xf6, 0xd5, 0x11, 0x33, 0x13, 0x10, 0x00, 0x60, 0xa6, 0xbb, 0xa6, 0x47,
        0xb5,
    ]);
    tables.extend_from_slice(&[0; 12]);
    u32b(&mut tables, 32);
    for n in 5..=10 {
        u32b(&mut tables, if n == 5 { 0xff00ff00 } else { 0 });
        u32b(&mut tables, 4);
    }
    u32b(&mut tables, 18);
    tables.extend_from_slice(&[0; 72 + 16]);
    let mut meta = vec![];
    text(&mut meta, "RSe Meta Stream Version 8");
    meta.extend_from_slice(&8u16.to_le_bytes());
    meta.extend_from_slice(&[0; 16]);
    wide(&mut meta, "PmBRepSegment");
    meta.extend_from_slice(&[1; 16]);
    meta.extend_from_slice(&[0; 12]);
    text(&mut meta, "");
    text(&mut meta, "");
    meta.push(0);
    meta.extend(compress(&tables, false));
    c.create_stream("/RSeStorage/Mone")
        .unwrap()
        .write_all(&meta)
        .unwrap();
    if duplicate_meta {
        c.create_stream("/RSeStorage/Mtwo")
            .unwrap()
            .write_all(&meta)
            .unwrap();
    }
    let mut bulk = vec![];
    u32b(&mut bulk, 0);
    bulk.extend_from_slice(&carrier);
    u32b(&mut bulk, carrier.len() as u32);
    bulk.push(0);
    u32b(&mut bulk, u32::MAX);
    let mut stream = vec![0; 18];
    stream.extend(compress(&bulk, zstd));
    c.create_stream("/RSeStorage/Bone")
        .unwrap()
        .write_all(&stream)
        .unwrap();
    if decoy {
        c.create_stream("/RSeStorage/Bdecoy")
            .unwrap()
            .write_all(&sab())
            .unwrap();
    }
    c.into_inner().into_inner()
}
#[test]
fn structural_carrier_selection_ignores_signature_decoys_and_preserves_spans() {
    for zstd in [false, true] {
        let d = read(
            &container(zstd, false, true),
            "fixture.ipt",
            &Limits::default(),
        )
        .unwrap();
        assert!(d.model.is_some(), "{:?}", d.summary.diagnostics);
        let info = d.summary.carrier.unwrap();
        assert_eq!(info.stream, "/RSeStorage/Bone");
        assert_eq!(info.codec, if zstd { "zstd" } else { "zlib" });
        let m = d.model.unwrap();
        assert_eq!(m.metadata().units_mm, Some(10.0));
        let raw = m.entities()[0].raw();
        let span = raw.source.as_ref().unwrap();
        assert_eq!(
            raw.raw_data.as_deref().unwrap(),
            &d.kernel_bytes.unwrap()
                [span.start_offset - info.kernel_offset..span.end_offset - info.kernel_offset]
        );
    }
}
#[test]
fn duplicate_identity_is_ambiguous_not_a_first_match() {
    let d = read(&container(false, true, false), "test", &Limits::default()).unwrap();
    assert!(d.model.is_none());
    assert!(d.summary.diagnostics[0].contains("found 2"));
}
#[test]
fn invalid_cfb_and_limits_are_reported() {
    assert!(read(b"ACIS BinaryFile fake", "test", &Limits::default()).is_err());
    let bytes = container(false, false, false);
    for limits in [
        Limits {
            max_file_bytes: 1,
            ..Default::default()
        },
        Limits {
            max_streams: 1,
            ..Default::default()
        },
    ] {
        assert!(read(&bytes, "test", &limits).is_err());
    }
    for limits in [
        Limits {
            max_stream_bytes: 1,
            ..Default::default()
        },
        Limits {
            max_inflated_bytes: 1,
            ..Default::default()
        },
        Limits {
            max_records: 0,
            ..Default::default()
        },
    ] {
        let d = read(&bytes, "test", &limits).unwrap();
        assert!(d.model.is_none());
        assert!(!d.summary.diagnostics.is_empty());
    }
}

#[test]
fn hard_limits_reject_overflow_before_reading() {
    let cap = serde_json::to_value(Limits::default()).unwrap();
    for (key, value) in cap.as_object().unwrap() {
        let mut changed = cap.clone();
        changed[key] = serde_json::json!(value.as_u64().unwrap() + 1);
        let limits: Limits = serde_json::from_value(changed).unwrap();
        assert!(read(b"not a container", "fuzz", &limits)
            .err()
            .unwrap()
            .0
            .contains("hard limit"));
    }
    assert!(Limits {
        max_file_bytes: usize::MAX,
        ..Limits::default()
    }
    .validate()
    .is_err());
}

#[test]
fn streaming_inflate_handles_empty_chunks_and_expansion_bombs() {
    for zstd in [false, true] {
        assert_eq!(
            rse::inflate(&compress(&[], zstd), 0).unwrap().0,
            Vec::<u8>::new()
        );
        for size in [16383, 16384, 16385, 32768, 65537] {
            let data: Vec<_> = (0..size).map(|i| (i * 31) as u8).collect();
            let encoded = compress(&data, zstd);
            assert_eq!(rse::inflate(&encoded, size).unwrap().0, data);
            assert!(rse::inflate(&encoded, size - 1).is_err());
        }
        let bomb = compress(&vec![0; 2 * 1024 * 1024], zstd);
        assert!(rse::inflate(&bomb, 32).unwrap_err().0.contains("limit"));
    }
}

#[test]
fn deterministic_cfb_header_and_sector_mutations_never_panic() {
    let valid = container(false, false, false);
    let limits = Limits {
        max_file_bytes: 1024 * 1024,
        max_inflated_bytes: 65536,
        max_total_inflated_bytes: 131072,
        max_streams: 128,
        max_records: 1024,
        ..Limits::default()
    };
    for offset in (0..512).step_by(4).chain((512..valid.len()).step_by(512)) {
        for replacement in [0u32, u32::MAX, 0x7fff_ffff] {
            let mut mutated = valid.clone();
            mutated[offset..offset + 4].copy_from_slice(&replacement.to_le_bytes());
            let _ = read(&mutated, "mutation", &limits);
            let _ = assembly::inspect(&mutated, "mutation", &limits);
        }
    }
}
#[test]
fn compressed_members_reject_truncation_concatenation_and_bombs() {
    for zstd in [false, true] {
        let encoded = compress(&vec![42; 4096], zstd);
        assert_eq!(rse::inflate(&encoded, 4096).unwrap().0.len(), 4096);
        assert!(rse::inflate(&encoded, 4095).is_err());
        for cut in 0..encoded.len() {
            assert!(
                rse::inflate(&encoded[..cut], 4096).is_err(),
                "accepted truncated {zstd} {cut}"
            );
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(rse::inflate(&trailing, 8192).is_err());
        let mut concat = encoded.clone();
        concat.extend(encoded);
        assert!(rse::inflate(&concat, 8192).is_err());
    }
}
#[test]
fn type_indices_and_record_lengths_must_match() {
    let meta = rse::Meta {
        id: [0; 16],
        name: String::new(),
        blocks: vec![0x8000_0001],
        types: vec![[0; 16]],
        inflated_bytes: 0,
        compressed_offset: 0,
        codec: "zlib",
        state_words: [0; 3],
    };
    let mut data = vec![0, 0, 0, 0, 42, 1, 0, 0, 0, 0, 255, 255, 255, 255];
    assert!(rse::records(&data, &meta, 25).is_ok());
    data[0] = 1;
    assert!(rse::records(&data, &meta, 25).is_err());
    data[0] = 0;
    data[5] = 2;
    assert!(rse::records(&data, &meta, 25).is_err());
    data[5] = 1;
    data[10] = 0;
    assert!(rse::records(&data, &meta, 25).is_err());
}

#[test]
fn unsupported_kernel_keeps_the_structurally_selected_bytes_and_provenance() {
    for version in [99999u32, 23200] {
        let mut file =
            cfb::CompoundFile::open(Cursor::new(container(false, false, false))).unwrap();
        let mut bytes = Vec::new();
        file.open_stream("/RSeStorage/Bone")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let (mut expanded, _) = rse::inflate(&bytes[18..], 4096).unwrap();
        // Synthetic record: selector, 14-byte carrier prefix, then 15-byte SAB magic.
        expanded[33..37].copy_from_slice(&version.to_le_bytes());
        bytes.truncate(18);
        bytes.extend(compress(&expanded, false));
        file.create_stream("/RSeStorage/Bone")
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        let doc = read(
            &file.into_inner().into_inner(),
            "fixture",
            &Limits::default(),
        )
        .unwrap();
        assert!(doc.model.is_none());
        assert!(doc.kernel_bytes.is_some());
        let info = doc.summary.carrier.unwrap();
        assert_eq!(info.stream, "/RSeStorage/Bone");
        assert_eq!(
            info.save_version,
            if version == 23200 { Some(23200) } else { None }
        );
        assert!(doc.summary.diagnostics[0].contains(&version.to_string()));
    }
}

#[test]
fn metadata_survives_unknown_geometry_schema_and_missing_property_sets() {
    let mut file = cfb::CompoundFile::open(Cursor::new(container(false, false, false))).unwrap();
    file.set_storage_clsid("/", "4d29b490-49b2-11d0-93c3-7e0706000000".parse().unwrap())
        .unwrap();
    let mut db = vec![];
    file.open_stream("/RSeStorage/V24/RSeDb")
        .unwrap()
        .read_to_end(&mut db)
        .unwrap();
    db[16..20].copy_from_slice(&999u32.to_le_bytes());
    file.create_stream("/RSeStorage/V24/RSeDb")
        .unwrap()
        .write_all(&db)
        .unwrap();
    let bytes = file.into_inner().into_inner();
    for d in [
        inspect(&bytes, "x", &Limits::default()).unwrap(),
        read(&bytes, "x", &Limits::default()).unwrap(),
    ] {
        assert_eq!(d.summary.kind, "part");
        assert!(d.model.is_none());
        assert_eq!(d.summary.document.databases[0].schema, Some(999));
        assert_eq!(d.summary.document.stages.properties, "missing");
        assert_eq!(d.summary.document.stages.state, "unresolved");
    }
}
#[test]
fn metadata_inspection_never_reads_corrupt_bulk_and_kind_conflicts_fail_closed() {
    let mut file = cfb::CompoundFile::open(Cursor::new(container(false, false, false))).unwrap();
    file.create_stream("/RSeStorage/Bone")
        .unwrap()
        .write_all(b"invalid bulk")
        .unwrap();
    let bytes = file.into_inner().into_inner();
    let metadata = inspect(&bytes, "renamed.idw", &Limits::default()).unwrap();
    assert_eq!(metadata.summary.kind, "part");
    assert_eq!(metadata.summary.document.stages.geometry, "not_attempted");
    assert!(metadata
        .summary
        .document
        .diagnostics
        .iter()
        .all(|d| !d.code.starts_with("geometry.")));
    let mut file = cfb::CompoundFile::open(Cursor::new(container(false, false, false))).unwrap();
    file.set_storage_clsid("/", "e60f81e1-49b3-11d0-93c3-7e0706000000".parse().unwrap())
        .unwrap();
    let doc = read(&file.into_inner().into_inner(), "x", &Limits::default()).unwrap();
    assert!(doc.model.is_none());
    assert_eq!(doc.summary.document.identification.status, "conflicting");
}

#[test]
fn property_limits_and_future_profiles_preserve_document_and_geometry() {
    let mut file = cfb::CompoundFile::open(Cursor::new(container(false, false, false))).unwrap();
    let property = crate::property_tests::controlled_stream();
    file.create_stream("/\u{5}Properties")
        .unwrap()
        .write_all(&property)
        .unwrap();
    let bytes = file.into_inner().into_inner();
    for limits in [
        Limits {
            max_property_bytes: 1,
            ..Default::default()
        },
        Limits {
            max_property_items: 0,
            ..Default::default()
        },
    ] {
        let doc = read(&bytes, "x", &limits).unwrap();
        assert!(doc.model.is_some());
        assert_eq!(doc.summary.document.stages.properties, "partial");
        assert!(doc.summary.document.diagnostics.iter().any(|d| matches!(
            d.code.as_str(),
            "property.limit_exceeded" | "property.stream_unavailable"
        )));
    }
    let mut file = cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
    let mut future = property.clone();
    future[2..4].copy_from_slice(&2u16.to_le_bytes());
    file.create_stream("/\u{5}Properties")
        .unwrap()
        .write_all(&future)
        .unwrap();
    let doc = read(&file.into_inner().into_inner(), "x", &Limits::default()).unwrap();
    assert!(doc.model.is_some());
    assert_eq!(
        doc.summary.document.unparsed_property_streams[0].status,
        "unsupported"
    );
    assert_eq!(
        doc.summary.document.unparsed_property_streams[0]
            .raw_data
            .as_ref()
            .unwrap()
            .0,
        future
    );
}
