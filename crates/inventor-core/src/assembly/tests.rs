use super::*;
use crate::read::Reader;
use std::io::Write;

fn u16b(b: &mut Vec<u8>, n: u16) {
    b.extend(n.to_le_bytes());
}
fn u32b(b: &mut Vec<u8>, n: u32) {
    b.extend(n.to_le_bytes());
}
fn wide(b: &mut Vec<u8>, s: &str) {
    let chars: Vec<_> = s.encode_utf16().collect();
    u32b(b, chars.len() as u32);
    for ch in chars {
        u16b(b, ch);
    }
}

// Independently authored structures. They exercise format guards, not vendor truth.
pub(super) fn fixture(
    kind: &str,
    id: [u8; 16],
    refs: &[(&str, [u8; 16], u32)],
    occurrences: &[(u32, u32)],
) -> Vec<u8> {
    let mut b = vec![];
    u16b(&mut b, 15);
    u16b(&mut b, 27);
    for n in [
        31, 32, 18, 28, 3, 12, 7, 4, 1, 5, 1, 2, 7, 2, 3, 7, 0, 5, 5, 0, 1, 0, 1, 0, 3, 2, 2,
    ] {
        u16b(&mut b, n);
    }
    b.extend([0, 0, 31, 64, 0, 0, 0, 0]);
    b.extend([0; 24]);
    wide(&mut b, "");
    b.extend([0; 32]);
    b.extend([9; 16]);
    u32b(&mut b, 0);
    b.extend(id);
    wide(&mut b, "fixture");
    u16b(&mut b, 0);
    if kind == "part" {
        return b;
    }
    u32b(&mut b, 0);
    u32b(&mut b, 0);
    b.extend([0; 4]);
    u16b(&mut b, 0);
    wide(&mut b, "Default");
    wide(&mut b, "DesignView");
    b.extend([0; 4]);
    wide(&mut b, "Default");
    b.extend([0; 4]);
    u32b(&mut b, 2);
    u32b(&mut b, occurrences.len() as u32);
    u16b(&mut b, 0);
    u16b(&mut b, 1);
    u32b(&mut b, refs.len() as u32);
    u32b(&mut b, 0);
    b.extend([0; 32]);
    u32b(&mut b, 0);
    u32b(&mut b, refs.len() as u32);
    wide(&mut b, "");
    u32b(&mut b, 0);
    for &(path, id, rid) in refs {
        wide(&mut b, path);
        u32b(&mut b, u32::MAX);
        wide(&mut b, "");
        u16b(&mut b, 0);
        wide(&mut b, "");
        u32b(&mut b, 0);
        b.extend([0; 4]);
        b.extend(id);
        b.extend([9; 16]);
        u32b(&mut b, rid);
        u32b(
            &mut b,
            occurrences.iter().filter(|(_, r)| *r == rid).count() as u32,
        );
        u32b(&mut b, 1);
        u32b(&mut b, 1);
    }
    b.push(0);
    u32b(&mut b, 0);
    b.push(0);
    u32b(&mut b, occurrences.len() as u32);
    for &(oid, rid) in occurrences {
        for n in [0, rid, oid, 0, 0] {
            u32b(&mut b, n);
        }
        u16b(&mut b, 0x2080);
        for n in [0, 1, 0, 0, 0, 0, 0, 0] {
            u32b(&mut b, n);
        }
        b.extend([0; 11]);
        u32b(&mut b, 0);
    }
    if occurrences.is_empty() {
        u32b(&mut b, 0);
    }
    b.extend([0x44; 4]);
    b
}

pub(super) fn container(
    kind: &str,
    id: [u8; 16],
    refs: &[(&str, [u8; 16], u32)],
    occurrences: &[(u32, u32)],
) -> Vec<u8> {
    let mut c = cfb::CompoundFile::create(Cursor::new(vec![])).unwrap();
    c.set_storage_clsid(
        "/",
        if kind == "assembly" {
            "e60f81e1-49b3-11d0-93c3-7e0706000000"
        } else {
            "4d29b490-49b2-11d0-93c3-7e0706000000"
        }
        .parse()
        .unwrap(),
    )
    .unwrap();
    c.create_storage_all("/RSeStorage/V24").unwrap();
    let mut db = vec![0; 16];
    u32b(&mut db, 31);
    db.extend([0; 32]);
    wide(&mut db, "");
    c.create_stream("/RSeStorage/V24/RSeDb")
        .unwrap()
        .write_all(&db)
        .unwrap();
    c.create_stream("/RSeStorage/RSeSegInfo")
        .unwrap()
        .write_all(&[0; 16])
        .unwrap();
    c.create_stream("/UFRxDoc")
        .unwrap()
        .write_all(&fixture(kind, id, refs, occurrences))
        .unwrap();
    c.into_inner().into_inner()
}

#[test]
fn ufrx_retains_ids_repetition_and_unknown_tail() {
    let bytes = fixture(
        "assembly",
        [1; 16],
        &[("parts/box.ipt", [2; 16], 7)],
        &[(42, 7), (99, 7)],
    );
    let mut r = Reader::new(&bytes);
    let mut doc = UfrxDocument::default();
    ufrx::parse(&mut r, "fixture", "assembly", &Default::default(), &mut doc).unwrap();
    assert_eq!(doc.references[0].occurrence_count, 2);
    assert_eq!(
        doc.occurrences
            .iter()
            .map(|o| o.occurrence_id)
            .collect::<Vec<_>>(),
        vec![42, 99]
    );
    assert_eq!(&bytes[r.pos..], &[0x44; 4]);
    assert!(doc.active_model_state.is_none());
    assert_eq!(
        doc.document_id.as_deref(),
        Some("01010101-0101-0101-0101-010101010101")
    );
}

#[test]
fn occurrence_property_values_preserve_tags_and_bounded_sources() {
    let mut bytes = fixture("assembly", [1; 16], &[("part.ipt", [2; 16], 7)], &[(42, 7)]);
    let mut header = UfrxDocument::default();
    ufrx::parse(
        &mut Reader::new(&bytes),
        "fixture",
        "assembly",
        &Default::default(),
        &mut header,
    )
    .unwrap();
    let start = header.occurrences[0].source.start_offset + 34;
    assert_eq!(&bytes[start..start + 8], &[0; 8]);
    let mut section = vec![];
    u32b(&mut section, 123);
    u32b(&mut section, 4);
    let mut encoded_string = vec![];
    wide(&mut encoded_string, "Observed string");
    for (tag, value) in [
        (5, encoded_string),
        (7, vec![0x80]),
        (0x19, 987u32.to_le_bytes().to_vec()),
        (0x2e, vec![0xab; 16]),
    ] {
        section.extend([1, tag]);
        u32b(&mut section, 456);
        section.push(tag);
        section.extend(value);
        u32b(&mut section, 789);
    }
    bytes.splice(start..start + 8, section);
    let mut doc = UfrxDocument::default();
    ufrx::parse(
        &mut Reader::new(&bytes),
        "fixture",
        "assembly",
        &Default::default(),
        &mut doc,
    )
    .unwrap();
    let properties = &doc.occurrences[0].properties;
    assert_eq!(properties.len(), 4);
    assert_eq!(
        properties[0].value,
        PropertyValue::String("Observed string".into())
    );
    assert_eq!(properties[1].value, PropertyValue::Byte(0x80));
    assert_eq!(properties[2].value, PropertyValue::Integer(987));
    assert_eq!(properties[3].value, PropertyValue::Bytes16([0xab; 16]));
    assert!(doc.occurrences[0].title.is_none()); // No name inference from tag 5.
    for p in properties {
        assert_eq!(
            (
                p.section_index,
                p.section_header,
                p.header_value,
                p.trailer_value
            ),
            (0, 123, 456, 789)
        );
        assert!(p.flag);
        assert_eq!(bytes[p.source.start_offset + 1], p.tag);
        assert!(p.source.end_offset <= doc.occurrences[0].source.end_offset);
    }
    // A cut in the second value must retain the first complete property only.
    let end = properties[1].source.end_offset - 1;
    let mut cut = UfrxDocument::default();
    assert!(ufrx::parse(
        &mut Reader::new(&bytes[..end]),
        "fixture",
        "assembly",
        &Default::default(),
        &mut cut
    )
    .is_err());
    assert_eq!(cut.occurrences[0].properties.len(), 1);
    assert_ne!(cut.status, "decoded_subset");
    assert_eq!(
        cut.occurrences[0].source.end_offset,
        properties[0].source.end_offset
    );
}

#[test]
fn every_truncated_ufrx_prefix_is_bounded_and_not_complete() {
    let bytes = fixture("assembly", [1; 16], &[("box.ipt", [2; 16], 7)], &[(42, 7)]);
    for end in 0..bytes.len() - 4 {
        let mut r = Reader::new(&bytes[..end]);
        let mut doc = UfrxDocument::default();
        assert!(
            ufrx::parse(&mut r, "fixture", "assembly", &Default::default(), &mut doc).is_err(),
            "cut {end}"
        );
        assert_ne!(doc.status, "decoded_subset");
    }
}

#[test]
fn nearby_profiles_are_not_admitted_and_aggregate_counts_are_bounded() {
    let bytes = fixture("assembly", [1; 16], &[("box.ipt", [2; 16], 7)], &[(42, 7)]);
    for offset in [0, 4, 8, 4 + 27 * 2 + 2] {
        let mut bad = bytes.clone();
        bad[offset] += 1;
        assert!(ufrx::parse(
            &mut Reader::new(&bad),
            "fixture",
            "assembly",
            &Default::default(),
            &mut UfrxDocument::default()
        )
        .is_err());
    }
    assert!(ufrx::parse(
        &mut Reader::new(&bytes),
        "fixture",
        "assembly",
        &Limits {
            max_property_items: 1,
            ..Default::default()
        },
        &mut UfrxDocument::default()
    )
    .is_err());
}

#[test]
fn unresolved_identity_does_not_drop_occurrences_or_invent_placement() {
    let doc = inspect(
        &container(
            "assembly",
            [1; 16],
            &[("missing.ipt", [2; 16], 7)],
            &[(42, 7), (42, 7)],
        ),
        "fixture",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(doc.status, "partial");
    assert_eq!(doc.occurrences.len(), 2);
    assert!(doc
        .occurrences
        .iter()
        .all(|o| o.local_transform_mm.is_none()));
    assert!(doc
        .occurrences
        .iter()
        .all(|o| o.visible.is_none() && o.substitute.is_none()));
}

#[test]
fn compact_masks_preserve_negative_one_and_reject_nonfinite_or_projective() {
    let mut bytes = vec![];
    u32b(&mut bytes, 0x203);
    // Rz(90) and explicit translation (2,3,4) in source cm.
    let expected = [
        [0., -1., 0., 2.],
        [1., 0., 0., 3.],
        [0., 0., 1., 4.],
        [0., 0., 0., 1.],
    ];
    let mut set = 0u16;
    let mut zero = 0u16;
    let mut values = vec![];
    for (i, &x) in expected.iter().flatten().enumerate() {
        if x == 1. || x == -1. {
            set |= 1 << i;
        }
        if x == 0. || x == -1. {
            zero |= 1 << i;
        }
        if x != 0. && x != 1. && x != -1. {
            values.extend(f64::to_le_bytes(x));
        }
    }
    u16b(&mut bytes, set);
    u16b(&mut bytes, zero);
    bytes.extend(values);
    assert_eq!(
        records::compact(&mut Reader::new(&bytes)).unwrap(),
        (true, [set, zero], expected)
    );
    for bad in [f64::NAN, f64::INFINITY] {
        let mut b = bytes.clone();
        b[8..16].copy_from_slice(&bad.to_le_bytes());
        assert!(records::compact(&mut Reader::new(&b)).is_err());
    }
    let mut projective = IDENTITY;
    projective[3][0] = 0.5;
    assert!(compose(&IDENTITY, &projective).is_err());
}

#[test]
fn matrix_composition_is_parent_times_local_and_rejects_overflow() {
    let p = [
        [0., -1., 0., 10.],
        [1., 0., 0., 20.],
        [0., 0., 1., 30.],
        [0., 0., 0., 1.],
    ];
    let mut local = IDENTITY;
    local[0][3] = 2.;
    local[1][3] = 3.;
    let w = compose(&p, &local).unwrap();
    assert_eq!([w[0][3], w[1][3], w[2][3]], [7., 22., 30.]);
    assert_ne!(w, compose(&local, &p).unwrap());
    let mut huge = IDENTITY;
    huge[0][3] = f64::MAX;
    assert!(compose(&huge, &huge).is_err());
}
