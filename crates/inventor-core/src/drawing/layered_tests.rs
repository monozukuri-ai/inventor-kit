//! Synthetic Meta6/7 (majors 11-14) records: inline layer markers, dense
//! record numbering and the older registry/Meta grammars. No production bytes.
use super::*;
use crate::{rse, Limits};
use std::io::{Cursor, Write};

/// A record payload. Layered writers end class layers and nested sections
/// with a u32 length marker, like the Meta6/7 wire; Meta8 writers do not.
struct Wire {
    bytes: Vec<u8>,
    layered: bool,
    sections: Vec<(usize, u32)>,
}
impl Wire {
    fn new(layered: bool) -> Self {
        Self {
            bytes: vec![],
            layered,
            sections: vec![],
        }
    }
    fn word(&mut self, n: u32) -> &mut Self {
        self.bytes.extend(n.to_le_bytes());
        self
    }
    fn short(&mut self, n: u16) -> &mut Self {
        self.bytes.extend(n.to_le_bytes());
        self
    }
    fn byte(&mut self, n: u8) -> &mut Self {
        self.bytes.push(n);
        self
    }
    fn zeros(&mut self, n: usize) -> &mut Self {
        self.bytes.extend(vec![0; n]);
        self
    }
    fn floats(&mut self, values: &[f32]) -> &mut Self {
        for v in values {
            self.bytes.extend(v.to_le_bytes());
        }
        self
    }
    fn doubles(&mut self, values: &[f64]) -> &mut Self {
        for v in values {
            self.bytes.extend(v.to_le_bytes());
        }
        self
    }
    fn wide(&mut self, s: &str) -> &mut Self {
        self.word(s.encode_utf16().count() as u32);
        for c in s.encode_utf16() {
            self.bytes.extend(c.to_le_bytes());
        }
        self
    }
    fn refs(&mut self, values: &[u32]) -> &mut Self {
        self.word(0x30000002).word(values.len() as u32);
        if !values.is_empty() {
            self.word(values.len() as u32).word(0);
        }
        for v in values {
            self.word(*v);
        }
        self
    }
    fn layer(&mut self) -> &mut Self {
        if self.layered {
            let at = self.bytes.len() as u32;
            self.sections.push((0, at));
            self.word(at);
        }
        self
    }
    fn open(&self) -> usize {
        self.bytes.len()
    }
    fn close(&mut self, start: usize) -> &mut Self {
        if self.layered {
            let length = (self.bytes.len() - start) as u32;
            self.sections.push((start, length));
            self.word(length);
        }
        self
    }
    /// Block-table markers: sections in pre-order.
    fn markers(&self) -> Vec<u32> {
        let mut sections = self.sections.clone();
        sections.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        sections.into_iter().map(|(_, length)| length).collect()
    }
}

fn decode(
    major: u8,
    kind: &str,
    type_id: &str,
    bytes: &[u8],
    layers: Option<&[u32]>,
) -> crate::Result<Option<PayloadObservation>> {
    let mut source = SourceSpan::stream("synthetic", "/RSeStorage/Btest", 0, bytes.len());
    source.byte_domain = "inflated_stream";
    super::fields::decode(
        kind,
        super::fields::Context::from(major).layers(layers),
        type_id,
        3,
        bytes,
        source,
        &mut 100_000,
    )
}

fn value<'a>(o: &'a PayloadObservation, name: &str) -> &'a FieldValue {
    &o.fields.iter().find(|f| f.name == name).unwrap().value
}

/// Decodes `w` with its own markers, then checks that every marker value,
/// the block-table list, truncation and the Meta8 reading all fail closed.
fn exact(major: u8, kind: &str, type_id: &str, w: &Wire, finish: bool) -> PayloadObservation {
    let markers = w.markers();
    assert!(!markers.is_empty());
    let o = decode(major, kind, type_id, &w.bytes, Some(&markers))
        .unwrap_or_else(|e| panic!("major {major} {type_id}: {e}"))
        .unwrap();
    assert_eq!(o.layout, super::profile::get(major).unwrap().fields_name());
    for (start, length) in &w.sections {
        let at = start + *length as usize;
        let mut bad = w.bytes.clone();
        bad[at] ^= 1;
        assert!(
            decode(major, kind, type_id, &bad, Some(&markers)).is_err(),
            "{type_id} marker at {at}"
        );
    }
    // The list must be exactly the record's markers in pre-order.
    let mut extra = markers.clone();
    extra.push(4);
    let mut missing = markers.clone();
    missing.pop();
    for list in [extra, missing, vec![]] {
        assert!(decode(major, kind, type_id, &w.bytes, Some(&list)).is_err());
    }
    if markers.windows(2).any(|p| p[0] != p[1]) {
        let mut reordered = markers.clone();
        reordered.reverse();
        assert!(decode(major, kind, type_id, &w.bytes, Some(&reordered)).is_err());
    }
    // Without the Meta6/7 table the markers are unexplained bytes.
    assert!(decode(major, kind, type_id, &w.bytes, None).is_err());
    if finish {
        for n in 0..w.bytes.len() {
            assert!(decode(major, kind, type_id, &w.bytes[..n], Some(&markers)).is_err());
        }
        let mut longer = w.bytes.clone();
        longer.push(0);
        assert!(decode(major, kind, type_id, &longer, Some(&markers)).is_err());
    }
    o
}

const DL: &str = "DlSheetDlSegmentType";
const SM: &str = "DlSheetSmSegmentType";
const LINE: &str = "a79eacc7-11d1-c281-6000-a38ab46bceb0";
const POINTS: &str = "a79eaccb-11d1-c281-6000-a38ab46bceb0";
const GROUP: &str = "a79eaccf-11d1-c281-6000-a38ab46bceb0";
const TEXT: &str = "a79eacd5-11d1-c281-6000-a38ab46bceb0";
const TRIANGLES: &str = "a79eacd2-11d1-c281-6000-a38ab46bceb0";
const MARKER: &str = "41305114-11d2-6450-6000-b4856c2387b0";

fn header(w: &mut Wire, object: u16, attribute: u32, owner: u32) {
    w.word(0).short(object).layer();
    w.word(0)
        .word(attribute)
        .word(0)
        .word(owner)
        .word(0)
        .layer();
}

#[test]
fn layered_display_primitives_keep_the_meta8_fields() {
    for major in 11..=14 {
        let mut w = Wire::new(true);
        header(&mut w, 7, 0x12, 0x80000002);
        w.doubles(&[1., 2., 0., 3., 4., 0.]);
        let o = exact(major, DL, LINE, &w, true);
        assert_eq!(o.proposed_role, "stored_line_candidate");
        assert!(matches!(value(&o, "object_id"), FieldValue::U16(7)));
        assert!(matches!(value(&o, "attribute_reference"), FieldValue::U32(v) if v == &[0x12]));
        assert!(matches!(value(&o, "owner_reference"), FieldValue::U32(v) if v == &[0x80000002]));
        // The Meta8 reading of the same fields has no markers.
        let mut plain = Wire::new(false);
        header(&mut plain, 7, 0x12, 0x80000002);
        plain.doubles(&[1., 2., 0., 3., 4., 0.]);
        assert!(decode(major, DL, LINE, &plain.bytes, Some(&[])).is_err());

        let mut w = Wire::new(true);
        header(&mut w, 8, 0, 0);
        w.refs(&[0x80000004, 0]).byte(0);
        assert_eq!(
            exact(major, DL, GROUP, &w, true).proposed_role,
            "display_group_candidate"
        );

        // Point lists close each Point3f; major11 omits the zero suffix.
        let mut w = Wire::new(true);
        header(&mut w, 9, 0, 0x80000001);
        w.word(0x30000002).word(2).word(2).word(0x102);
        for p in [[0f32, 0., 0.], [5., 6., 0.]] {
            let s = w.open();
            w.floats(&p).close(s);
        }
        if major > 11 {
            w.byte(0);
        }
        let o = exact(major, DL, POINTS, &w, true);
        assert!(
            matches!(value(&o, "xyz_points_candidate"), FieldValue::F32(v) if v == &[0., 0., 0., 5., 6., 0.])
        );

        // Text: majors 11 and 12 end after the style index.
        let mut w = Wire::new(true);
        header(&mut w, 10, 0, 0x80000001);
        w.wide("A1").doubles(&[1., 2., 0., 1., 0., 0.]);
        w.short(9).word(0x21);
        if major > 12 {
            w.byte(0);
        }
        let o = exact(major, DL, TEXT, &w, true);
        assert!(matches!(value(&o, "text"), FieldValue::Utf16(s) if s == "A1"));

        // Triangle batches with index flags 0, vertices closed one by one.
        let mut w = Wire::new(true);
        header(&mut w, 11, 0, 0x80000001);
        w.word(0x30000002).word(3).word(3).word(0x102);
        for p in [[0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
            let s = w.open();
            w.floats(&p).close(s);
        }
        w.word(0x30000002)
            .word(3)
            .word(3)
            .word(0)
            .word(0)
            .word(1)
            .word(2);
        for n in [0x30000002, 0, 0x30000002, 0, 0x105, 0x30000002, 0, 0, 0] {
            w.word(n);
        }
        assert_eq!(
            exact(major, SM, TRIANGLES, &w, true).proposed_role,
            "stored_triangles_candidate"
        );

        // Point markers end a layer nine (major11) or ten bytes into their tail.
        let mut w = Wire::new(true);
        w.word(0).short(12).layer();
        w.word(0x40000005)
            .word(0)
            .word(0)
            .word(0x80000001)
            .word(0)
            .layer();
        w.doubles(&[1., 2., 0.]).floats(&[1., 0.]);
        if major == 11 {
            w.byte(0);
        } else {
            w.short(0);
        }
        w.layer().word(0);
        assert_eq!(
            exact(major, SM, MARKER, &w, true).proposed_role,
            "stored_point_marker_candidate"
        );
    }
}

#[test]
fn layered_style_records_nest_their_sections() {
    let list = "48eb8607-11d2-070c-6000-f99ac5361ab0";
    let boolean = "b32bf6a3-11d2-09f4-6000-f99ac5361ab0";
    let binding = "f2fb355d-42d4-07c8-a07b-e085474d8be7";
    let stroke = "b32bf6ac-11d2-09f4-6000-f99ac5361ab0";
    let color = "48eb8608-11d2-070c-6000-f99ac5361ab0";
    for major in 11..=14 {
        // Attribute lists open with an empty base layer.
        let mut w = Wire::new(true);
        w.layer();
        w.word(0x30000002)
            .word(2)
            .word(2)
            .word(0x10000000)
            .word(5)
            .word(6);
        let o = exact(major, DL, list, &w, true);
        assert_eq!(o.fields.len(), 2);

        let mut w = Wire::new(true);
        w.layer().word(4).layer().byte(0);
        exact(major, DL, boolean, &w, true);

        // Major11 layer bindings have no flags byte.
        let mut w = Wire::new(true);
        w.layer()
            .word(1)
            .layer()
            .word(3)
            .word(u32::MAX)
            .floats(&[1.]);
        if major > 11 {
            w.byte(0);
        }
        let o = exact(major, DL, binding, &w, true);
        assert!(matches!(value(&o, "layer_reference"), FieldValue::U32(v) if v == &[3]));

        let mut w = Wire::new(true);
        w.layer().word(8).layer();
        let s = w.open();
        w.short(0)
            .floats(&[0.5])
            .short(1)
            .short(0)
            .byte(0)
            .short(2)
            .short(2);
        w.doubles(&[0.4, -0.1]).floats(&[-10000., 1., 0.]).close(s);
        w.word(0).word(1);
        let o = exact(major, DL, stroke, &w, true);
        assert!(matches!(value(&o, "stroke_dashes"), FieldValue::F64(v) if v == &[0.4, -0.1]));

        // One RGBA section, then a section of four RGBA sections and a value.
        let mut w = Wire::new(true);
        w.layer().word(1).layer();
        let s = w.open();
        w.floats(&[0.25, 0.5, 0.75, 1.]).close(s);
        let rest = w.open();
        for n in 0..4 {
            let s = w.open();
            w.floats(&[n as f32 / 4., 0., 0., 1.]).close(s);
        }
        w.floats(&[20.]).close(rest).byte(1);
        let o = exact(major, DL, color, &w, true);
        assert!(matches!(value(&o, "color_rgba_parameters"),
            FieldValue::F32(v) if v.len() == 21 && v[..4] == [0.25, 0.5, 0.75, 1.] && v[20] == 20.));
    }
}

#[test]
fn layered_directory_and_layer_tables_follow_their_major() {
    let fonts = "3e9f410e-11d2-6481-6000-708a806bceb0";
    let layer = "c1ab98dd-4ed8-5d1f-22c5-51841631b75e";
    for major in 11..=14 {
        let profile = super::profile::get(major).unwrap();
        let (floats, flags) = profile.font_tail();
        assert_eq!((floats, flags), if major == 11 { (1, 0) } else { (2, 3) });
        let mut w = Wire::new(true);
        w.word(0).short(2).layer();
        w.word(0x30000002).word(2).word(2).word(0x102);
        for id in [1, 2] {
            let entry = w.open();
            w.word(id);
            let font = w.open();
            w.short(4)
                .short(400)
                .short(0)
                .floats(&[0.25, 0.])
                .wide("Synthetic");
            w.floats(&vec![0.; floats]).zeros(flags);
            w.close(font).close(entry);
        }
        w.layer().word(3);
        let o = exact(major, "DlDirectorySegmentType", fonts, &w, true);
        assert_eq!(o.fields.len(), 2 * (7 + flags) + 1);

        let mut w = Wire::new(true);
        w.word(0).short(19).layer();
        w.zeros(profile.layer_prefix)
            .wide("Synthetic layer")
            .zeros(6)
            .wide("");
        w.layer().doubles(&[0.35]).word(0x6dc4);
        let s = w.open();
        w.floats(&[0., 0., 0., 1.]).close(s);
        w.byte(1).doubles(&[1.]).zeros(profile.layer_flags());
        let o = exact(major, "AppSegmentType", layer, &w, true);
        assert_eq!(
            o.fields
                .iter()
                .filter(|f| f.name.starts_with("layer_flag_"))
                .count(),
            if major == 11 { 1 } else { 3 }
        );
    }
}

fn placement(w: &mut Wire, display: u32, definition: u32) {
    w.word(0).short(5).layer().zeros(8).byte(1).layer();
    w.word(0x30000003).word(0).layer();
    w.word(display).layer().byte(1).layer();
    w.word(definition).short(0x8421).short(0x7bde);
}

#[test]
fn layered_sheet_placements_and_views_frame_like_major15() {
    let space = "f4a2f948-11d1-7bd2-0008-7abdec18db09";
    let border = "62a8e6a8-11d1-ad4b-6000-108a806bceb0";
    let local = "5e4e86c7-11d0-fe3f-6000-0dbd351c3cb0";
    let leader = "b86459e3-11d4-f88c-1000-cdab7dd247b5";
    let view = "8a6d1381-11d1-6b56-6000-38bd861c3cb0";
    for major in 11..=14 {
        let mut w = Wire::new(true);
        placement(&mut w, 1, 2);
        w.byte(1).layer().word(2).doubles(&[0., 0., 42., 29.7]);
        let o = exact(major, SM, space, &w, true);
        assert!(
            matches!(value(&o, "origin_and_extent_candidate"), FieldValue::F64(v) if v == &[0., 0., 42., 29.7])
        );

        let mut w = Wire::new(true);
        placement(&mut w, 3, 4);
        w.byte(1).layer().word(4).layer().word(4);
        exact(major, SM, border, &w, true);

        // Local displays close an empty layer after their definition. The
        // local annotation and leader displays admitted for majors 15-22 frame
        // with the same layered layout.
        let mut w = Wire::new(true);
        w.word(0).short(6).layer().zeros(8).byte(1).layer();
        w.word(0x30000003).word(0).layer().word(0x80000004).layer();
        w.word(7).byte(1).layer().layer();
        for local in [
            local,
            "c0ca9b69-11d2-54d6-6000-4ab209e1b5b0",
            "648cd16a-11d1-c06c-6000-24b209e1b5b0",
            "45a1b92d-11d2-6538-6000-4fb209e1b5b0",
        ] {
            let o = exact(major, SM, local, &w, true);
            assert_eq!(o.proposed_role, "sheet_local_display_candidate");
        }
        w.word(7);
        for leader in [leader, "8deaf986-11d4-3763-6000-61b782b6fbb0"] {
            let o = exact(major, SM, leader, &w, true);
            assert_eq!(o.proposed_role, "sheet_local_display_candidate");
        }

        // Views: 16-bit flags, a separator, no suffix GUID; the trailing
        // unresolved byte is absent on major11.
        let mut w = Wire::new(true);
        placement(&mut w, 9, 10);
        w.byte(1).layer().word(10).layer();
        w.short(0).refs(&[]).short(0x100).refs(&[]).wide("VIEW1");
        w.word(0).word(1).doubles(&[0.; 6]);
        if major > 11 {
            w.byte(0);
        }
        let o = exact(major, SM, view, &w, true);
        assert!(matches!(value(&o, "view_name_unresolved"), FieldValue::Utf16(s) if s == "VIEW1"));
    }
}

#[test]
fn layered_document_prefixes_and_opaque_suffix_layers_are_checked() {
    let document = "d37c90cb-11d0-fa16-6000-0dbd861c3cb0";
    let name = "d37c90cd-11d0-fa16-6000-0dbd861c3cb0";
    let links = "a200fb76-11d1-6107-0008-70bdec18db09";
    for major in 11..=14 {
        let spans = super::profile::get(major).unwrap().document_spans();
        assert_eq!(
            spans,
            if major <= 12 {
                [16, 4, 16]
            } else {
                [12, 8, 16]
            }
        );
        let mut w = Wire::new(true);
        w.word(0).short(0x1e).layer().zeros(8).layer().zeros(16);
        w.wide("Synthetic").refs(&[]);
        for span in spans {
            w.zeros(span).layer();
        }
        w.refs(&[0x80000007, 0x80000008]);
        let o = exact(major, "DlDocDcSegmentType", document, &w, false);
        assert!(matches!(value(&o, "sheet_references_candidate"),
            FieldValue::U32(v) if v == &[0x80000007, 0x80000008]));
        // The other major's boundaries do not match these markers.
        let other = if major <= 12 { 13 } else { 12 };
        assert!(decode(
            other,
            "DlDocDcSegmentType",
            document,
            &w.bytes,
            Some(&w.markers())
        )
        .is_err());

        // A prefix observation leaves later bytes opaque; their class-layer
        // markers must still be found at their own offsets.
        let mut w = Wire::new(true);
        w.word(0)
            .short(2)
            .layer()
            .zeros(8)
            .layer()
            .zeros(16)
            .wide("Sheet");
        w.zeros(10).layer().zeros(3).layer().layer().zeros(5);
        let markers = w.markers();
        let o = decode(
            major,
            "DlSheetDcSegmentType",
            name,
            &w.bytes,
            Some(&markers),
        )
        .unwrap()
        .unwrap();
        assert_eq!(o.proposed_role, "sheet_name_candidate");
        for (start, length) in &w.sections {
            let at = start + *length as usize;
            let mut bad = w.bytes.clone();
            bad[at] ^= 1;
            assert!(decode(major, "DlSheetDcSegmentType", name, &bad, Some(&markers)).is_err());
        }
        let mut nested = markers.clone();
        nested.push(2);
        assert!(decode(major, "DlSheetDcSegmentType", name, &w.bytes, Some(&nested)).is_err());

        // An unframed sheet definition keeps its object key only when every
        // marker of the record is a class layer at its own offset.
        let mut w = Wire::new(true);
        w.word(5)
            .short(0x8a77)
            .layer()
            .zeros(8)
            .layer()
            .zeros(8)
            .layer();
        w.zeros(4)
            .layer()
            .layer()
            .zeros(4)
            .refs(&[])
            .layer()
            .zeros(9);
        let markers = w.markers();
        let o = decode(major, "DlDocDcSegmentType", links, &w.bytes, Some(&markers))
            .unwrap()
            .unwrap();
        assert_eq!(o.layout, "idw-sheet-links-prefix-fallback");
        let mut bad = w.bytes.clone();
        bad[6] ^= 1;
        assert!(decode(major, "DlDocDcSegmentType", links, &bad, Some(&markers)).is_err());
    }
}

#[test]
fn layered_rgba_view_caches_close_the_object_key_layer() {
    let bitmap = "02f8872d-11d6-938b-1000-4d979f8d7ab5";
    for major in 11..=14 {
        let mut w = Wire::new(true);
        w.word(0)
            .short(15)
            .layer()
            .byte(1)
            .byte(1)
            .word(2)
            .word(3)
            .word(0)
            .byte(0);
        w.zeros(2 * 3 * 4).byte(1);
        let o = exact(major, SM, bitmap, &w, true);
        assert!(matches!(value(&o, "bitmap_height_width"), FieldValue::U32(v) if v == &[2, 3]));
    }
}

// ---------------------------------------------------------------------------
// Containers: Meta6/7 block tables, the Meta6 header and older registries.

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
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}
const TYPE_A: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

/// A Meta stream with the given block table, one type and empty sections.
fn meta_stream(version: u16, name: &str, id: [u8; 16], blocks: &[u32]) -> Vec<u8> {
    typed_meta_stream(version, name, id, blocks, TYPE_A)
}
fn typed_meta_stream(
    version: u16,
    name: &str,
    id: [u8; 16],
    blocks: &[u32],
    type_id: [u8; 16],
) -> Vec<u8> {
    let mut body = vec![0; 14];
    word(&mut body, blocks.len() as u32);
    for b in blocks {
        word(&mut body, *b);
    }
    word(&mut body, 4 + 4 * blocks.len() as u32);
    for _ in 0..2 {
        word(&mut body, 0);
        word(&mut body, 4);
    }
    word(&mut body, 1);
    body.extend(type_id);
    body.extend([0; 12]);
    word(&mut body, 32);
    for n in 5..=10 {
        word(&mut body, if n == 5 { 0xff00ff00 } else { 0 });
        word(&mut body, 4);
    }
    word(&mut body, 18);
    body.extend([0; 88]);
    let mut out = vec![];
    text(&mut out, &format!("RSe Meta Stream Version {version}"));
    out.extend(version.to_le_bytes());
    out.extend([0; 16]);
    wide(&mut out, name);
    out.extend(id);
    out.extend([0; 12]);
    if version == 6 {
        word(&mut out, 7);
        text(&mut out, "");
        word(&mut out, 0x4000_0000);
    } else {
        text(&mut out, "");
    }
    text(&mut out, "01/01/2000 00:00:00");
    out.push(1);
    out.extend(zlib(&body));
    out
}

/// Records (selector, payload) framed without extended trailers.
fn bulk_body(records: &[&[u8]]) -> Vec<u8> {
    let mut out = vec![];
    for r in records {
        word(&mut out, 0x100);
        out.extend(*r);
        word(&mut out, r.len() as u32);
    }
    word(&mut out, u32::MAX);
    out
}

#[test]
fn meta6_and_meta7_block_tables_number_records_densely() {
    let limits = Limits::default();
    let blocks = [0x8000000c, 6, 0x80000004, 0x80000010, 8, 0];
    for version in [6u16, 7] {
        let bytes = meta_stream(version, "DLSheet1DLSegment", [1; 16], &blocks);
        assert_eq!(rse::meta_stream_version(&bytes).unwrap(), version);
        let meta = rse::meta(&bytes, &limits).unwrap();
        assert_eq!(meta.version, version);
        assert_eq!(meta.timestamps[1], "01/01/2000 00:00:00");
        let body = bulk_body(&[&[0; 12], &[0; 4], &[0; 16]]);
        let table = rse::layered_record_table(&body, &meta, 14, &mut 100, true).unwrap();
        let shape: Vec<_> = table
            .records
            .iter()
            .map(|r| (r.ordinal, r.block, r.layers.clone()))
            .collect();
        assert_eq!(shape, [(0, 0, vec![6]), (1, 2, vec![]), (2, 3, vec![8, 0])]);
        // Meta8 framing keeps the words as slots.
        let plain = rse::record_table(&body, &meta, 14, &mut 100, true).unwrap();
        assert_eq!(
            plain.records.iter().map(|r| r.ordinal).collect::<Vec<_>>(),
            [0, 2, 3]
        );
        // Layered framing charges records, not marker words.
        let mut work = 3;
        assert!(rse::layered_record_table(&body, &meta, 14, &mut work, true).is_ok());
        assert_eq!(work, 0);
        // A marker before any record, or longer than its record, fails closed.
        for bad in [
            vec![6, 0x8000000c, 0x80000004, 0x80000010],
            vec![0x8000000c, 9, 0x80000004, 0x80000010],
            vec![0x8000000c, 6, 0x80000004, 1, 0x80000010],
        ] {
            let meta = rse::meta(&meta_stream(version, "x", [1; 16], &bad), &limits).unwrap();
            assert!(rse::layered_record_table(&body, &meta, 14, &mut 100, true).is_err());
        }
    }
    let meta = rse::meta(&meta_stream(8, "x", [1; 16], &blocks), &limits).unwrap();
    let body = bulk_body(&[&[0; 12], &[0; 4], &[0; 16]]);
    assert!(rse::layered_record_table(&body, &meta, 14, &mut 100, true).is_err());
    let mut wrong = meta_stream(6, "x", [1; 16], &blocks);
    let at = wrong.windows(2).position(|w| w == b"6\x06").unwrap();
    wrong[at] = b'5';
    wrong[at + 1] = 5;
    assert!(rse::meta(&wrong, &limits).is_err());
}

#[test]
fn meta7_drawing_tables_bound_records_not_marker_words() {
    // Twenty records with one marker each: forty block words.
    let blocks: Vec<u32> = (0..20).flat_map(|_| [0x80000010, 4]).collect();
    let bytes = meta_stream(7, "x", [1; 16], &blocks);
    let limits = |max_records| Limits {
        max_records,
        ..Limits::default()
    };
    assert!(rse::meta(&bytes, &limits(20)).is_err());
    let layered = rse::MetaLayout {
        layered_blocks: true,
        ..rse::MetaLayout::STANDARD
    };
    let parse = |bytes: &[u8], limits: &Limits| {
        rse::meta_layout_budgeted(bytes, limits, &mut { 1 << 20 }, &mut { 1000 }, layered)
    };
    assert_eq!(parse(&bytes, &limits(20)).unwrap().blocks.len(), 40);
    let error = parse(&bytes, &limits(19)).err().unwrap();
    assert!(
        error.to_string().contains("Meta6/7 record count"),
        "{error}"
    );
    // Meta8 tables keep bounding every word.
    assert!(parse(&meta_stream(8, "x", [1; 16], &blocks), &limits(20)).is_err());
}

fn registry(schema: u32, major: u8, nodes: usize) -> Vec<u8> {
    let mut out = vec![];
    word(&mut out, 1);
    wide(&mut out, "DLSheet1DLSegment");
    out.extend([1; 16]);
    out.extend([0; 16]);
    word(&mut out, 0);
    word(&mut out, 1);
    out.extend([0; 24]);
    wide(&mut out, "DlSheetDlSegmentType");
    out.extend([0; 8]);
    out.extend([0, 0, major, 0x80, 0, 0, 0, 0]);
    word(&mut out, 0);
    out.extend([0; 16 + 9 + 16 + 4]);
    word(&mut out, nodes as u32 + 1);
    for n in 0..nodes {
        word(&mut out, n as u32);
        out.extend(0u16.to_le_bytes());
        out.extend(vec![0; if schema == 29 { 12 } else { 16 }]);
    }
    word(&mut out, 1);
    for count in [2, 1] {
        word(&mut out, count);
        for _ in 0..count {
            if schema == 31 {
                out.extend([2; 16]);
            } else {
                wide(&mut out, "SMxDisplayList");
            }
        }
    }
    out
}

#[test]
fn older_registries_skip_class_name_tails_and_shorter_nodes() {
    use rse::RegistryGrammar;
    for (schema, grammar) in [
        (31, RegistryGrammar::Schema31),
        (30, RegistryGrammar::Schema30),
        (29, RegistryGrammar::Schema29),
    ] {
        assert!(RegistryGrammar::for_schema(Some(schema)) == grammar);
        let bytes = registry(schema, 13, 3);
        let segments = rse::registry_with_grammar(&bytes, &mut 1000, grammar).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].major, 13);
        for other in [
            RegistryGrammar::Schema31,
            RegistryGrammar::Schema30,
            RegistryGrammar::Schema29,
        ] {
            if other != grammar {
                assert!(rse::registry_with_grammar(&bytes, &mut 1000, other).is_err());
            }
        }
        for n in 0..bytes.len() {
            assert!(rse::registry_with_grammar(&bytes[..n], &mut 1000, grammar).is_err());
        }
    }
}

fn container(clsid: &str, schema: u32, major: u8, meta_version: u16) -> Vec<u8> {
    type Cfb = cfb::CompoundFile<Cursor<Vec<u8>>>;
    let mut c = Cfb::create(Cursor::new(vec![])).unwrap();
    c.set_storage_clsid("/", clsid.parse().unwrap()).unwrap();
    c.create_storage_all("/RSeStorage/V1").unwrap();
    let mut put = |path: &str, data: &[u8]| c.create_stream(path).unwrap().write_all(data).unwrap();
    let mut db = vec![0; 16];
    word(&mut db, schema);
    db.extend([0; 32]);
    wide(&mut db, "");
    put("/RSeStorage/V1/RSeDb", &db);
    put("/RSeStorage/RSeSegInfo", &registry(schema, major, 0));
    let mut w = Wire::new(true);
    header(&mut w, 1, 0, 0);
    w.doubles(&[0., 0., 0., 1., 1., 0.]);
    let mut blocks = vec![0x80000000 | w.bytes.len() as u32];
    blocks.extend(w.markers());
    // Type a79eacc7-11d1-c281-6000-a38ab46bceb0 (stored line).
    let line = [
        0xc7, 0xac, 0x9e, 0xa7, 0xd1, 0x11, 0x81, 0xc2, 0x60, 0x00, 0xa3, 0x8a, 0xb4, 0x6b, 0xce,
        0xb0,
    ];
    put(
        "/RSeStorage/Mone",
        &typed_meta_stream(meta_version, "DLSheet1DLSegment", [1; 16], &blocks, line),
    );
    let mut bulk = profile::BULK_HEADER.to_vec();
    bulk[17] = 1;
    bulk.extend(zlib(&bulk_body(&[&w.bytes])));
    put("/RSeStorage/Bone", &bulk);
    c.into_inner().into_inner()
}

#[test]
fn schemas_29_and_30_are_drawing_only_and_profiles_require_their_meta_version() {
    const DRAWING: &str = "bbf9fdf1-52dc-11d0-8c04-0800090be8ec";
    const PART: &str = "4d29b490-49b2-11d0-93c3-7e0706000000";
    for (schema, major, version) in [(29, 11, 6), (30, 12, 7), (30, 13, 7), (31, 14, 7)] {
        let data = container(DRAWING, schema, major, version);
        let d = inspect(&data, "synthetic.idw", &Limits::default()).unwrap();
        assert_eq!(d.metadata.databases[0].status, "decoded");
        assert_eq!(d.metadata.segments[0].major, major);
        let s = &d.segments[0];
        assert_eq!(s.status, "framed", "{:?}", s.diagnostics);
        assert_eq!(s.records.len(), 1);
        assert_eq!(s.records[0].ordinal, 0);
        // The block source covers the length word and both layer markers.
        let b = &s.records[0].meta_block_source;
        assert_eq!(b.end_offset - b.start_offset, 12);
        assert_eq!(s.observations[0].proposed_role, "stored_line_candidate");
        // A profile admits only the Meta version it was observed with, and
        // rejects the others before any decompression.
        for other in [6u16, 7, 8] {
            if other == version {
                continue;
            }
            let data = container(DRAWING, schema, major, other);
            let d = inspect(&data, "synthetic.idw", &Limits::default()).unwrap();
            let s = &d.segments[0];
            assert_eq!(s.status, "unavailable");
            assert!(s.records.is_empty());
            assert_eq!(s.diagnostics[0].code, "drawing.meta_framing_unavailable");
            assert_eq!(d.usage.expanded_bytes, 0);
        }
        if schema != 31 {
            let doc = crate::inspect(
                &container(PART, schema, major, version),
                "x.ipt",
                &Limits::default(),
            )
            .unwrap();
            assert_eq!(doc.summary.document.databases[0].status, "unsupported");
            assert!(doc.summary.document.segments.is_empty());
        }
    }
    for schema in [21, 26, 28] {
        let data = container(DRAWING, schema, 13, 7);
        let d = inspect(&data, "synthetic.idw", &Limits::default()).unwrap();
        assert_eq!(d.metadata.databases[0].status, "unsupported");
        assert!(d.segments.is_empty());
    }
}
