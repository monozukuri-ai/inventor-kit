use crate::{
    property::{self, Budget, PropertySet, PropertyValue as V},
    Limits,
};
fn u32b(b: &mut Vec<u8>, n: u32) {
    b.extend(n.to_le_bytes());
}
fn pad(b: &mut Vec<u8>) {
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}
fn typed(vt: u16, payload: &[u8]) -> Vec<u8> {
    let mut b = vec![];
    u32b(&mut b, vt as u32);
    b.extend(payload);
    pad(&mut b);
    b
}
fn wide(value: &str) -> Vec<u8> {
    let units = value.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut b = vec![];
    u32b(&mut b, units.len() as u32);
    for c in units {
        b.extend(c.to_le_bytes());
    }
    pad(&mut b);
    b
}
fn section(entries: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut b = vec![0; 8 + entries.len() * 8];
    b[4..8].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    for (i, (pid, value)) in entries.iter().enumerate() {
        let offset = b.len() as u32;
        b[8 + i * 8..12 + i * 8].copy_from_slice(&pid.to_le_bytes());
        b[12 + i * 8..16 + i * 8].copy_from_slice(&offset.to_le_bytes());
        b.extend(value);
        pad(&mut b);
    }
    let size = b.len() as u32;
    b[..4].copy_from_slice(&size.to_le_bytes());
    b
}
fn stream(sections: &[Vec<u8>], version: u16) -> Vec<u8> {
    let mut b = vec![0; 28 + 20 * sections.len()];
    b[..2].copy_from_slice(&[0xfe, 0xff]);
    b[2..4].copy_from_slice(&version.to_le_bytes());
    b[24..28].copy_from_slice(&(sections.len() as u32).to_le_bytes());
    for (i, s) in sections.iter().enumerate() {
        b[28 + i * 20] = i as u8 + 1;
        let offset = b.len() as u32;
        b[44 + i * 20..48 + i * 20].copy_from_slice(&offset.to_le_bytes());
        b.extend(s);
    }
    b
}
fn parse(b: &[u8]) -> (Vec<PropertySet>, Vec<crate::document::Diagnostic>) {
    let mut d = vec![];
    let s = property::parse(
        b,
        "fixture",
        "/\u{5}properties",
        &mut Budget::new(&Limits::default()),
        &mut d,
    )
    .unwrap();
    (s, d)
}
fn cp(cp: u16) -> (u32, Vec<u8>) {
    (1, typed(2, &cp.to_le_bytes()))
}
fn get(set: &PropertySet, pid: u32) -> &V {
    set.properties
        .iter()
        .find(|p| p.pid == pid)
        .unwrap()
        .value
        .as_ref()
        .unwrap()
}

#[test]
fn cfb_stream_paths_preserve_root_and_member_properties_on_every_host() {
    use std::{io::Cursor, io::Write, path::Path};

    let mut file =
        cfb::CompoundFile::open(Cursor::new(crate::tests::container(false, false, false))).unwrap();
    let properties = [
        ("/\u{5}Tracking", "ROOT-001"),
        ("/MemberDocs/部品/\u{5}Tracking/CONTENTS", "MEMBER-002"),
    ];
    for (path, value) in properties {
        let mut data = stream(&[section(&[cp(1200), (5, typed(31, &wide(value)))])], 0);
        // Design Tracking FMTID, PID 5: part_number.
        data[28..44].copy_from_slice(&[
            0x0f, 0x3f, 0x85, 0x32, 0x44, 0x34, 0xd1, 0x11, 0x9e, 0x93, 0x00, 0x60, 0xb0, 0x3c,
            0x1c, 0xa6,
        ]);
        file.create_storage_all(Path::new(path).parent().unwrap())
            .unwrap();
        file.create_stream(path).unwrap().write_all(&data).unwrap();
    }
    let bytes = file.into_inner().into_inner();
    let doc = crate::inspect(&bytes, "portable-paths", &Limits::default()).unwrap();
    let info = &doc.summary.document;
    assert_eq!(info.stages.properties, "decoded");
    assert_eq!(info.stages.registry, "decoded");
    assert_eq!(info.databases[0].stream, "/RSeStorage/V24/RSeDb");
    assert_eq!(info.property_sets.len(), properties.len());
    for (path, value) in properties {
        let set = info
            .property_sets
            .iter()
            .find(|set| set.source.stream == path)
            .unwrap();
        let prop = set.properties.iter().find(|p| p.pid == 5).unwrap();
        assert_eq!(prop.semantic_name, Some("part_number"));
        assert_eq!(prop.source.stream, path);
        assert_eq!(
            get(set, 5),
            &V::Text {
                value: value.into()
            }
        );
    }
    assert!(doc
        .summary
        .streams
        .iter()
        .all(|s| s.path.starts_with('/') && !s.path.contains('\\')));
}

#[test]
fn unicode_dictionary_codepages_and_original_spans() {
    let mut dict = vec![];
    u32b(&mut dict, 1);
    u32b(&mut dict, 77);
    dict.extend(wide("製品名"));
    let first = section(&[(77, typed(31, &wide("部品🔩\0A"))), (0, dict), cp(1200)]);
    let mut latin = vec![];
    u32b(&mut latin, 5);
    latin.extend(b"caf\xe9\0");
    let second = section(&[(8, typed(30, &latin)), cp(1252)]);
    let bytes = stream(&[first, second], 1);
    let (sets, d) = parse(&bytes);
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(
        get(&sets[0], 77),
        &V::Text {
            value: "部品🔩\0A".into()
        }
    );
    assert_eq!(sets[0].properties[0].name.as_deref(), Some("製品名"));
    assert_eq!(
        get(&sets[1], 8),
        &V::Text {
            value: "café".into()
        }
    );
    for set in &sets {
        for p in &set.properties {
            assert_eq!(
                p.raw_data.0,
                bytes[p.source.start_offset..p.source.end_offset]
            );
        }
    }
}
#[test]
fn shift_jis_and_utf8_are_explicit_and_do_not_replace_bad_bytes() {
    for (code, characters, expected) in [
        (932, vec![0x93, 0xfa, 0x96, 0x7b, 0], "日本"),
        (65001, "日本\0".as_bytes().to_vec(), "日本"),
    ] {
        let mut b = vec![];
        u32b(&mut b, characters.len() as u32);
        b.extend(characters);
        let (s, d) = parse(&stream(&[section(&[cp(code), (5, typed(30, &b))])], 0));
        assert!(d.is_empty());
        assert_eq!(
            get(&s[0], 5),
            &V::Text {
                value: expected.into()
            }
        );
    }
    let mut b = vec![];
    u32b(&mut b, 2);
    b.extend([0x80, 0]);
    let (s, d) = parse(&stream(&[section(&[cp(65001), (5, typed(30, &b))])], 0));
    assert_eq!(s[0].properties[1].status, "malformed");
    assert_eq!(d[0].code, "property.value_invalid");
}
#[test]
fn scalar_types_keep_exact_integer_time_binary_and_decimal_domains() {
    let mut blob = vec![];
    u32b(&mut blob, 3);
    blob.extend([0, 128, 255]);
    let mut decimal = vec![0, 0, 28, 128];
    decimal.extend(0xffff_ffffu32.to_le_bytes());
    decimal.extend(u64::MAX.to_le_bytes());
    let entries = vec![
        cp(1200),
        (2, typed(2, &(-2i16).to_le_bytes())),
        (3, typed(21, &u64::MAX.to_le_bytes())),
        (4, typed(64, &132_000_000_000_000_007u64.to_le_bytes())),
        (5, typed(7, &(-1.25f64).to_le_bytes())),
        (6, typed(65, &blob)),
        (7, typed(14, &decimal)),
        (8, typed(11, &0xffffu16.to_le_bytes())),
        (9, typed(72, &[0; 16])),
    ];
    let (s, d) = parse(&stream(&[section(&entries)], 1));
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(get(&s[0], 2), &V::Signed { value: -2 });
    assert_eq!(get(&s[0], 3), &V::Unsigned { value: u64::MAX });
    assert_eq!(
        get(&s[0], 4),
        &V::Filetime {
            ticks_100ns: 132_000_000_000_000_007
        }
    );
    assert_eq!(get(&s[0], 5), &V::OleDate { days: -1.25 });
    assert_eq!(
        get(&s[0], 6),
        &V::Blob {
            data: property::Binary(vec![0, 128, 255])
        }
    );
    assert!(matches!(
        get(&s[0], 7),
        V::Decimal {
            scale: 28,
            negative: true,
            ..
        }
    ));
}
#[test]
fn packed_vectors_variant_vectors_and_array_bounds() {
    let mut packed = vec![];
    u32b(&mut packed, 3);
    for n in [-2i16, 3, 4] {
        packed.extend(n.to_le_bytes());
    }
    let mut variants = vec![];
    u32b(&mut variants, 2);
    variants.extend(typed(31, &wide("abc")));
    variants.extend(typed(11, &0xffffu16.to_le_bytes()));
    let mut array = vec![];
    for n in [3u32, 2, 2, (-2i32) as u32, 2, 5, 10, 20, 30, 40] {
        u32b(&mut array, n);
    }
    let (s, d) = parse(&stream(
        &[section(&[
            cp(1200),
            (2, typed(0x1002, &packed)),
            (3, typed(0x100c, &variants)),
            (4, typed(0x2003, &array)),
        ])],
        1,
    ));
    assert!(d.is_empty(), "{d:?}");
    assert!(
        matches!(get(&s[0],2),V::Sequence{values,..} if values==&[V::Signed{value:-2},V::Signed{value:3},V::Signed{value:4}])
    );
    assert!(
        matches!(get(&s[0],3),V::Sequence{values,..} if values==&[V::Text{value:"abc".into()},V::Bool{value:true}])
    );
    assert!(
        matches!(get(&s[0],4),V::Sequence{dimensions,values,..} if dimensions[0].lower_bound==-2 && dimensions[1].lower_bound==5 && values.len()==4)
    );
}
#[test]
fn unknown_types_missing_codepage_and_bad_values_do_not_hide_other_properties() {
    let mut text = vec![];
    u32b(&mut text, 2);
    text.extend(b"x\0");
    let (s, d) = parse(&stream(
        &[section(&[
            (2, typed(0xeeee, &[1, 2, 3, 4])),
            (3, typed(30, &text)),
            (4, typed(31, &wide("independent"))),
            (5, typed(11, &1u16.to_le_bytes())),
            (6, typed(5, &f64::NAN.to_le_bytes())),
            (7, typed(31, &[0xff; 4])),
        ])],
        0,
    ));
    assert_eq!(s[0].status, "partial");
    assert_eq!(s[0].properties[0].status, "unsupported");
    assert_eq!(s[0].properties[0].raw_data.0, typed(0xeeee, &[1, 2, 3, 4]));
    assert_eq!(
        get(&s[0], 4),
        &V::Text {
            value: "independent".into()
        }
    );
    assert!(d.iter().all(|e| e.source.is_some()));
    assert!(d.iter().any(|e| e.code == "property.code_page_missing"));
}
#[test]
fn every_truncation_and_overlapping_or_duplicate_tables_are_rejected() {
    let bytes = stream(
        &[section(&[cp(1200), (9, typed(31, &wide("boundary")))])],
        0,
    );
    for cut in 0..bytes.len() {
        assert!(
            property::parse(
                &bytes[..cut],
                "x",
                "/p",
                &mut Budget::new(&Limits::default()),
                &mut vec![]
            )
            .is_err(),
            "{cut}"
        );
    }
    for at in [56usize, 60] {
        let mut changed = bytes.clone();
        let source = if at == 56 { 64 } else { 68 };
        changed[at..at + 4].copy_from_slice(&bytes[source..source + 4]);
        let (s, d) = parse(&changed);
        assert_eq!(s[0].status, "malformed");
        assert_eq!(d[0].code, "property.section_invalid");
    }
    let mut overlap = stream(&[section(&[cp(1200)]), section(&[cp(1252)])], 0);
    let offset = overlap[44..48].to_vec();
    overlap[64..68].copy_from_slice(&offset);
    assert!(property::parse(
        &overlap,
        "x",
        "/p",
        &mut Budget::new(&Limits::default()),
        &mut vec![]
    )
    .is_err());
}
#[test]
fn sequence_budget_and_recursion_are_bounded() {
    let mut huge = vec![];
    u32b(&mut huge, u32::MAX);
    let (s, d) = parse(&stream(
        &[section(&[cp(1200), (8, typed(0x1000 | 17, &huge))])],
        1,
    ));
    assert_eq!(s[0].properties[1].status, "limited");
    assert!(d.iter().any(|e| e.code == "property.limit_exceeded"));
    let mut recursive = typed(0, &[]);
    for _ in 0..20 {
        let mut next = vec![];
        u32b(&mut next, 1);
        next.extend(recursive);
        recursive = typed(0x100c, &next);
    }
    let (s, d) = parse(&stream(&[section(&[cp(1200), (8, recursive)])], 1));
    assert_eq!(s[0].properties[1].status, "limited");
    assert!(d.iter().any(|e| e.message.contains("depth")));
}

pub(super) fn controlled_stream() -> Vec<u8> {
    stream(
        &[section(&[cp(1200), (77, typed(31, &wide("controlled")))])],
        0,
    )
}

#[test]
fn semantic_names_use_fmtid_and_pid_not_display_names() {
    let mut dictionary = vec![];
    u32b(&mut dictionary, 1);
    u32b(&mut dictionary, 4);
    dictionary.extend(wide("Part Number"));
    let mut bytes = stream(
        &[section(&[
            cp(1200),
            (0, dictionary),
            (4, typed(31, &wide("著者"))),
        ])],
        0,
    );
    bytes[28..44].copy_from_slice(&[
        0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3,
        0xd9,
    ]);
    let (sets, _) = parse(&bytes);
    let p = sets[0].properties.iter().find(|p| p.pid == 4).unwrap();
    assert_eq!(p.name.as_deref(), Some("Part Number"));
    assert_eq!(p.semantic_name, Some("author"));
    bytes[28..44].fill(0);
    let (sets, _) = parse(&bytes);
    assert!(sets[0].properties.iter().all(|p| p.semantic_name.is_none()));
}
