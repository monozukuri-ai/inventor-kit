use super::*;
use candidate::ReadOptions;
use std::io::Write;

fn compressed(data: &[u8]) -> Vec<u8> {
    let mut c = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    c.write_all(data).unwrap();
    c.finish().unwrap()
}
fn rewrite(c: &mut cfb::CompoundFile<Cursor<Vec<u8>>>, path: &str, data: &[u8]) {
    c.create_stream(path).unwrap().write_all(data).unwrap();
}
fn bytes(c: &mut cfb::CompoundFile<Cursor<Vec<u8>>>, path: &str) -> Vec<u8> {
    let mut b = vec![];
    c.open_stream(path).unwrap().read_to_end(&mut b).unwrap();
    b
}
/// Build controlled tables from independently specified carrier counts/flags.
/// These are mutation tests, not Inventor Model State oracle files.
fn carriers(flags: &[u8], history_only: bool) -> Vec<u8> {
    let mut c =
        cfb::CompoundFile::open(Cursor::new(tests::container(false, false, false))).unwrap();
    let mut m = bytes(&mut c, "/RSeStorage/Mone");
    let meta = rse::meta(&m, &Limits::default()).unwrap();
    let old = rse::inflate(&m[meta.compressed_offset..], 4096).unwrap().0;
    let mut table = old[..14].to_vec();
    table.extend_from_slice(&(flags.len() as u32).to_le_bytes());
    let b = bytes(&mut c, "/RSeStorage/Bone");
    let expanded = rse::inflate(&b[18..], 4096).unwrap().0;
    let record = rse::records(&expanded, &meta, 25).unwrap().remove(0);
    let template = &expanded[record.start..record.end];
    let mut bulk = vec![];
    for (i, &enabled) in flags.iter().enumerate() {
        let mut payload = template.to_vec();
        if history_only {
            let end = payload.len() - 18;
            let parsed =
                acis_core::sab::parse_sab(&payload[14..end], "fixture", &Default::default())
                    .unwrap();
            let mut kernel = payload[14..14 + parsed.header.data_offset].to_vec();
            kernel[27..31].copy_from_slice(&1u32.to_le_bytes());
            kernel.extend_from_slice(b"\x0d\x0bdelta_state");
            let footer = payload[end..].to_vec();
            payload.truncate(14);
            payload.extend(kernel);
            payload.extend(footer);
        }
        let n = payload.len();
        payload[n - 14] = enabled;
        payload[n - 18..n - 14].copy_from_slice(&(0x8000_0001u32 + i as u32).to_le_bytes());
        table.extend_from_slice(&(0x8000_0000u32 | n as u32).to_le_bytes());
        bulk.extend_from_slice(&0u32.to_le_bytes());
        bulk.extend_from_slice(&payload);
        bulk.extend_from_slice(&(n as u32).to_le_bytes());
        bulk.push(0);
    }
    table.extend_from_slice(&(4 + 4 * flags.len() as u32).to_le_bytes());
    table.extend_from_slice(&old[26..]); // old section 1: prefix14 + count4 + block4 + span4
    bulk.extend_from_slice(&u32::MAX.to_le_bytes());
    m.truncate(meta.compressed_offset);
    m.extend(compressed(&table));
    let mut b = vec![0; 18];
    b.extend(compressed(&bulk));
    rewrite(&mut c, "/RSeStorage/Mone", &m);
    rewrite(&mut c, "/RSeStorage/Bone", &b);
    c.into_inner().into_inner()
}
#[test]
fn multiple_typed_candidates_are_not_collapsed_and_explicit_ids_are_content_bound() {
    let data = carriers(&[1, 1], false);
    let limits = Limits::default();
    let inventory = inspect_candidates(&data, "a", &limits).unwrap();
    assert!(inventory.model.is_none());
    assert!(inventory.kernel_bytes.is_none());
    assert_eq!(inventory.summary.geometry.candidates.len(), 2);
    assert_eq!(inventory.summary.geometry.selection.status, "not_requested");
    let d = read(&data, "a", &limits).unwrap();
    assert!(d.model.is_none());
    assert_eq!(d.summary.geometry.selection.status, "ambiguous");
    for c in &inventory.summary.geometry.candidates {
        let options = ReadOptions {
            candidate_id: Some(c.id.clone()),
            ..Default::default()
        };
        let selected = read_with_options(&data, "renamed", &limits, &options).unwrap();
        assert!(selected.model.is_some());
        assert_eq!(
            selected.summary.geometry.selection.state_verification,
            "unverified"
        );
        assert_eq!(
            selected.summary.carrier.unwrap().record_ordinal,
            c.record_ordinal
        );
        assert_eq!(
            candidate::sha256(selected.kernel_bytes.as_ref().unwrap()),
            c.kernel_sha256.as_ref().unwrap().as_str()
        );
        let different =
            read_with_options(&carriers(&[1, 0], false), "a", &limits, &options).unwrap();
        assert!(different.model.is_none());
        assert_eq!(different.summary.geometry.selection.status, "not_found");
    }
}
#[test]
fn failed_candidates_and_unknown_flags_never_select_the_successful_neighbor() {
    let data = carriers(&[1, 3], false);
    let limits = Limits::default();
    let d = read(&data, "x", &limits).unwrap();
    assert!(d.model.is_none());
    assert_eq!(d.summary.geometry.candidates.len(), 2);
    assert_eq!(d.summary.geometry.selection.status, "ambiguous");
    assert!(d.summary.geometry.candidates[1].carrier.is_none());
    let options = ReadOptions {
        candidate_id: Some(d.summary.geometry.candidates[0].id.clone()),
        ..Default::default()
    };
    assert!(read_with_options(&data, "x", &limits, &options)
        .unwrap()
        .model
        .is_some());
    let data = carriers(&[0], false);
    let d = read(&data, "x", &limits).unwrap();
    assert!(d.model.is_none());
    assert_eq!(d.summary.geometry.selection.status, "state_unverified");
    let options = ReadOptions {
        candidate_id: Some(d.summary.geometry.candidates[0].id.clone()),
        ..Default::default()
    };
    let explicit = read_with_options(&data, "x", &limits, &options).unwrap();
    assert!(explicit.model.is_some());
    assert_eq!(
        explicit.summary.geometry.candidates[0].suppression,
        "unresolved"
    );
}
#[test]
fn history_only_needs_replay_and_current_state_requires_independent_evidence() {
    let limits = Limits::default();
    let data = carriers(&[1], true);
    let d = read(&data, "x", &limits).unwrap();
    assert!(d.model.is_none());
    assert!(d.kernel_bytes.is_some());
    let c = &d.summary.geometry.candidates[0];
    assert_eq!(c.table_status, "history_only");
    assert!(c.history_source.is_some());
    assert_eq!(c.history_replay, "required_unimplemented");
    let data = carriers(&[1], false);
    let options = ReadOptions {
        require_current_state: true,
        ..Default::default()
    };
    let d = read_with_options(&data, "x", &limits, &options).unwrap();
    assert!(d.model.is_none());
    assert_eq!(d.summary.geometry.selection.status, "state_unverified");
    let mut options = options;
    options.candidate_id = Some(d.summary.geometry.candidates[0].id.clone());
    assert!(read_with_options(&data, "x", &limits, &options)
        .unwrap()
        .model
        .is_none());
}
#[test]
fn multiple_databases_preserve_inventory_but_do_not_choose_a_database() {
    let mut c = cfb::CompoundFile::open(Cursor::new(carriers(&[1], false))).unwrap();
    let db = bytes(&mut c, "/RSeStorage/V24/RSeDb");
    c.create_storage_all("/RSeStorage/V25").unwrap();
    rewrite(&mut c, "/RSeStorage/V25/RSeDb", &db);
    let data = c.into_inner().into_inner();
    let d = read(&data, "x", &Limits::default()).unwrap();
    assert!(d.model.is_none());
    assert_eq!(d.summary.document.databases.len(), 2);
    assert_eq!(d.summary.document.segments.len(), 1);
    assert_eq!(d.summary.geometry.candidates.len(), 1);
    assert_eq!(d.summary.geometry.selection.status, "database_unresolved");
    let options = ReadOptions {
        candidate_id: Some(d.summary.geometry.candidates[0].id.clone()),
        ..Default::default()
    };
    let d = read_with_options(&data, "x", &Limits::default(), &options).unwrap();
    assert!(d.model.is_some());
    assert_eq!(
        d.summary.geometry.candidates[0].database_binding,
        "unresolved"
    );
}
#[test]
fn candidate_and_aggregate_expansion_budgets_prevent_automatic_partial_selection() {
    let data = carriers(&[1, 1], false);
    for limits in [
        Limits {
            max_candidates: 1,
            ..Default::default()
        },
        Limits {
            max_total_inflated_bytes: 1,
            ..Default::default()
        },
        Limits {
            max_records: 1,
            ..Default::default()
        },
    ] {
        let d = read(&data, "x", &limits).unwrap();
        assert!(d.model.is_none());
        assert_eq!(d.summary.geometry.status, "partial");
        assert!(d
            .summary
            .geometry
            .diagnostics
            .iter()
            .any(|d| d.message.contains("limit") || d.message.contains("exceeds")));
    }
}
#[test]
fn multiple_brep_segments_and_a_missing_pair_keep_other_candidates_visible() {
    let mut c = cfb::CompoundFile::open(Cursor::new(carriers(&[1], false))).unwrap();
    let r = bytes(&mut c, "/RSeStorage/RSeSegInfo");
    let entries = rse::registry(&r).unwrap();
    let mut second = r[entries[0].start_offset..entries[0].end_offset].to_vec();
    let offset = second.windows(16).position(|w| w == [1; 16]).unwrap();
    second[offset..offset + 16].fill(2);
    let mut registry = 2u32.to_le_bytes().to_vec();
    registry.extend_from_slice(&r[4..entries[0].end_offset]);
    registry.extend(second);
    registry.extend_from_slice(&r[entries[0].end_offset..]);
    rewrite(&mut c, "/RSeStorage/RSeSegInfo", &registry);
    let mut meta = bytes(&mut c, "/RSeStorage/Mone");
    let offset = meta.windows(16).position(|w| w == [1; 16]).unwrap();
    meta[offset..offset + 16].fill(2);
    rewrite(&mut c, "/RSeStorage/Mtwo", &meta);
    let bulk = bytes(&mut c, "/RSeStorage/Bone");
    rewrite(&mut c, "/RSeStorage/Btwo", &bulk);
    let data = c.into_inner().into_inner();
    let d = read(&data, "x", &Limits::default()).unwrap();
    assert!(d.model.is_none());
    assert_eq!(d.summary.geometry.candidates.len(), 2);
    assert_eq!(d.summary.geometry.selection.status, "ambiguous");
    let mut c = cfb::CompoundFile::open(Cursor::new(data)).unwrap();
    c.remove_stream("/RSeStorage/Btwo").unwrap();
    let data = c.into_inner().into_inner();
    let d = read(&data, "x", &Limits::default()).unwrap();
    assert!(d.model.is_none());
    assert_eq!(d.summary.geometry.candidates.len(), 1);
    assert_eq!(d.summary.geometry.selection.status, "incomplete");
    let options = ReadOptions {
        candidate_id: Some(d.summary.geometry.candidates[0].id.clone()),
        ..Default::default()
    };
    assert!(read_with_options(&data, "x", &Limits::default(), &options)
        .unwrap()
        .model
        .is_some());
}
