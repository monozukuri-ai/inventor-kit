//! Typed wire observations. Roles are hypotheses, never a qualified display list.
use super::{FieldObservation, FieldValue, PayloadObservation};
use crate::{document::SourceSpan, read::Reader, rse, Error, Result};

const POINT3F: [u8; 16] = [
    0xc1, 0x10, 0x72, 0xf2, 0xd2, 0x11, 0xa2, 0xcd, 0xa0, 0x00, 0x56, 0xb6, 0xfc, 0xdb, 0xc7, 0xc9,
];
pub(super) struct Context<'a> {
    major: u8,
    types: &'a [[u8; 16]],
}
impl<'a> Context<'a> {
    pub fn new(major: u8, types: &'a [[u8; 16]]) -> Self {
        Self { major, types }
    }
}
#[cfg(test)]
impl From<u8> for Context<'static> {
    fn from(major: u8) -> Self {
        Self::new(major, &[[0; 16], [0; 16], POINT3F])
    }
}

pub(super) struct Fields<'a, 'b> {
    pub profile: super::profile::Profile,
    types: &'a [[u8; 16]],
    pub r: Reader<'a>,
    pub work: &'b mut usize,
    source: SourceSpan,
    pub fields: Vec<FieldObservation>,
}
impl<'a, 'b> Fields<'a, 'b> {
    fn new(
        bytes: &'a [u8],
        source: SourceSpan,
        work: &'b mut usize,
        profile: super::profile::Profile,
        types: &'a [[u8; 16]],
    ) -> Self {
        Self {
            profile,
            types,
            r: Reader::new(bytes),
            work,
            source,
            fields: vec![],
        }
    }
    pub fn add(&mut self, name: &'static str, start: usize, value: FieldValue) {
        let mut source = self.source.clone();
        source.start_offset += start;
        source.end_offset = self.source.start_offset + self.r.pos;
        self.fields.push(FieldObservation {
            name,
            value,
            source,
        });
    }
    pub fn require(&mut self, expected: u32) -> Result<()> {
        if self.r.u32()? != expected {
            return Err(Error("unqualified drawing field layout".into()));
        }
        Ok(())
    }
    pub fn point_type(&mut self) -> Result<u32> {
        let selector = self.r.u32()?;
        if selector & !0xff != 0x100 || self.types.get((selector & 0xff) as usize) != Some(&POINT3F)
        {
            return Err(Error("unqualified drawing point type reference".into()));
        }
        Ok(selector)
    }
    pub fn word(&mut self, name: &'static str) -> Result<()> {
        rse::charge(self.work, 1)?;
        let start = self.r.pos;
        let value = self.r.u32()?;
        self.add(name, start, FieldValue::U32(vec![value]));
        Ok(())
    }
    pub fn byte(&mut self, name: &'static str) -> Result<()> {
        rse::charge(self.work, 1)?;
        let start = self.r.pos;
        let value = self.r.u8()?;
        self.add(name, start, FieldValue::U8(value));
        Ok(())
    }
    pub fn short(&mut self, name: &'static str) -> Result<()> {
        rse::charge(self.work, 1)?;
        let start = self.r.pos;
        let value = self.r.u16()?;
        self.add(name, start, FieldValue::U16(value));
        Ok(())
    }
    pub fn display_header(&mut self) -> Result<()> {
        self.word("header_flags")?;
        self.short("object_id")?;
        self.r.skip(4)?;
        self.word("attribute_reference")?;
        self.r.skip(4)?;
        self.word("owner_reference")?;
        self.r.skip(4)
    }
    pub fn floats(&mut self, name: &'static str, count: usize) -> Result<()> {
        rse::charge(self.work, count)?;
        let start = self.r.pos;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let v = f32::from_le_bytes(self.r.take(4)?.try_into().unwrap());
            if !v.is_finite() {
                return Err(Error("non-finite drawing field".into()));
            }
            values.push(v);
        }
        self.add(name, start, FieldValue::F32(values));
        Ok(())
    }
    pub fn text(&mut self, name: &'static str) -> Result<()> {
        let start = self.r.pos;
        let value = self.r.utf16()?;
        rse::charge(self.work, value.encode_utf16().count())?;
        self.add(name, start, FieldValue::Utf16(value));
        Ok(())
    }
    pub fn doubles(&mut self, name: &'static str, count: usize) -> Result<()> {
        rse::charge(self.work, count)?;
        let start = self.r.pos;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let v = f64::from_le_bytes(self.r.take(8)?.try_into().unwrap());
            if !v.is_finite() {
                return Err(Error("non-finite drawing field".into()));
            }
            values.push(v);
        }
        self.add(name, start, FieldValue::F64(values));
        Ok(())
    }
    pub fn references(&mut self, name: &'static str) -> Result<()> {
        let tag = self.r.u32()?;
        if tag != 0x30000002 && !(self.profile.compact_references && tag == 0x30000003) {
            return Err(Error("unqualified drawing reference list".into()));
        }
        let count = self.r.count(65536)?;
        rse::charge(self.work, count)?;
        if count != 0 {
            if tag == 0x30000002 && self.r.u32()? < count as u32 {
                return Err(Error("drawing list capacity below count".into()));
            }
            self.require(self.profile.reference_flags)?;
        }
        let start = self.r.pos;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.r.u32()?);
        }
        self.add(name, start, FieldValue::U32(values));
        Ok(())
    }
    pub fn compact(&mut self) -> Result<()> {
        rse::charge(self.work, 16)?;
        let start = self.r.pos;
        let prefixed =
            self.r.bytes.get(self.r.pos..self.r.pos + 4) == Some(&0x203u32.to_le_bytes());
        if prefixed {
            self.require(0x203)?;
        }
        let set = self.r.u16()?;
        let zero = self.r.u16()?;
        let mut values = [0.; 16];
        for (i, value) in values.iter_mut().enumerate() {
            *value = match (set & (1 << i) != 0, zero & (1 << i) != 0) {
                (false, false) => f64::from_le_bytes(self.r.take(8)?.try_into().unwrap()),
                (true, false) => 1.,
                (false, true) => 0.,
                (true, true) => -1.,
            };
            if !value.is_finite() {
                return Err(Error("non-finite compact drawing field".into()));
            }
        }
        self.add(
            "transform_candidate",
            start,
            FieldValue::CompactTransform {
                prefixed,
                set,
                zero,
                values,
            },
        );
        Ok(())
    }
}

pub(super) fn decode(
    kind: &str,
    context: Context<'_>,
    type_id: &str,
    ordinal: usize,
    bytes: &[u8],
    source: SourceSpan,
    work: &mut usize,
) -> Result<Option<PayloadObservation>> {
    let major = context.major;
    let profile = super::profile::get(major)
        .ok_or_else(|| Error("unsupported drawing field profile".into()))?;
    let decoder: fn(&mut Fields<'_, '_>) -> Result<()>;
    let role = match (kind, type_id) {
        ("DlDocDcSegmentType", "d37c90cb-11d0-fa16-6000-0dbd861c3cb0") => {
            decoder = super::sheet::document;
            "document_sheet_list_candidate"
        }
        ("DlSheetDcSegmentType", "d37c90cd-11d0-fa16-6000-0dbd861c3cb0") => {
            decoder = super::sheet::name;
            "sheet_name_candidate"
        }
        ("DlDocDcSegmentType", "a200fb76-11d1-6107-0008-70bdec18db09") => {
            decoder = super::sheet::links;
            "sheet_segment_links_candidate"
        }
        ("DlSheetSmSegmentType", "f4a2f948-11d1-7bd2-0008-7abdec18db09") => {
            decoder = super::sheet::space;
            "sheet_space_candidate"
        }
        (
            "DlSheetSmSegmentType",
            "d2d8dc28-11d1-ae0f-6000-108a806bceb0" | "82303d42-11d1-c02b-6000-188a806bceb0",
        ) => {
            decoder = super::sheet::placement;
            "sheet_placement_candidate"
        }
        ("DlSheetSmSegmentType", "8a6d1381-11d1-6b56-6000-38bd861c3cb0") if profile.views => {
            decoder = super::sheet::view_placement;
            "sheet_placement_candidate"
        }
        ("DlSheetSmSegmentType", "02f8872d-11d6-938b-1000-4d979f8d7ab5") => {
            decoder = super::sheet::view_bitmap;
            "stored_view_bitmap_candidate"
        }
        ("DlSheetSmSegmentType", "41305114-11d2-6450-6000-b4856c2387b0") => {
            decoder = super::sheet::point_marker;
            "stored_point_marker_candidate"
        }
        ("DlSheetSmSegmentType", "5741c02f-4467-1e22-0ba3-53bd0da0bc81") => {
            decoder = super::sheet::image;
            "stored_image_candidate"
        }
        ("DlSheetSmSegmentType", "576520b3-11d1-a496-6000-1b8aeb49cdb0") => {
            decoder = super::sheet::sketch_placement;
            "sheet_placement_candidate"
        }
        ("DlSheetSmSegmentType", "62a8e6a8-11d1-ad4b-6000-108a806bceb0")
            if matches!(major, 24 | 26 | 28) =>
        {
            decoder = super::sheet::border_placement;
            "sheet_placement_candidate"
        }
        (
            "DlSheetSmSegmentType",
            "5eb510c2-11d2-7068-6000-f191790357b0"
            | "5e4e86c7-11d0-fe3f-6000-0dbd351c3cb0"
            | "028c9254-11d1-e176-6000-2bb209e1b5b0",
        ) => {
            decoder = super::sheet::local_display;
            "sheet_local_display_candidate"
        }
        ("DlSheetSmSegmentType", "b86459e3-11d4-f88c-1000-cdab7dd247b5") => {
            decoder = super::sheet::leader_display;
            "sheet_local_display_candidate"
        }
        ("DlSheetSmSegmentType", "8deaf986-11d4-3763-6000-61b782b6fbb0")
            if matches!(major, 26 | 28) =>
        {
            decoder = super::sheet::leader_display;
            "sheet_local_display_candidate"
        }
        ("DlSheetSmSegmentType", "b2d41a36-4a3a-cc6b-07d7-8b8a7ece02ce") if major == 26 => {
            decoder = super::sheet::leader_display;
            "sheet_external_display_candidate"
        }
        (
            "DlSheetSmSegmentType",
            "f5a6ed7a-11d4-7c69-6000-69b782b6fbb0" | "35ecb98a-419c-f0d7-262b-dc98b8750e13",
        ) if major == 26 => {
            decoder = super::sheet::local_display;
            "sheet_local_display_candidate"
        }
        ("DlSheetSmSegmentType", "025e3388-4cbb-8851-7d1c-b0876dcb2a07")
            if profile.extended_record(type_id) =>
        {
            decoder = super::sheet::leader_display;
            "sheet_local_display_candidate"
        }
        (
            "DlSheetSmSegmentType",
            "c0ca9b69-11d2-54d6-6000-4ab209e1b5b0"
            | "648cd16a-11d1-c06c-6000-24b209e1b5b0"
            | "45a1b92d-11d2-6538-6000-4fb209e1b5b0",
        ) if matches!(major, 26 | 28) => {
            decoder = super::sheet::local_display;
            "sheet_local_display_candidate"
        }
        ("DlSheetSmSegmentType", "9b3499d1-11d1-8626-6000-27bd351c3cb0")
            if profile.extended_record(type_id) =>
        {
            decoder = super::sheet::local_display;
            "sheet_local_display_candidate"
        }
        ("DlSheetSmSegmentType", "05a6bf7b-45c2-fb50-9998-0ab04f9c8c86")
            if profile.extended_record(type_id) =>
        {
            decoder = super::sheet::leader_display;
            "sheet_external_display_candidate"
        }
        (
            "DlSheetSmSegmentType",
            "69c12b31-11d2-1c34-6000-1c9feb49cdb0" | "6589a70e-11d1-a4a7-6000-2fa5602d6bb0",
        ) if profile.extended_record(type_id) => {
            decoder = super::sheet::sketch_placement;
            "sheet_placement_candidate"
        }
        ("DlSheetSmSegmentType", "4e52b139-11d1-d3ba-6000-46bead9287b0")
            if profile.extended_record(type_id) =>
        {
            decoder = super::sheet::table_display;
            "sheet_local_transformed_display_candidate"
        }
        ("DlSheetSmSegmentType", "d0eee1ba-11d2-1cd5-0008-84ba1088db09") => {
            decoder = super::sheet::table_display;
            "sheet_local_transformed_display_candidate"
        }
        ("DlDirectorySegmentType", "3e9f410e-11d2-6481-6000-708a806bceb0") => {
            decoder = super::style::fonts;
            "font_table_candidate"
        }
        (
            "DlSheetDlSegmentType" | "DlSheetSmSegmentType",
            "48eb8607-11d2-070c-6000-f99ac5361ab0",
        ) => {
            decoder = super::style::attributes;
            "display_attributes_candidate"
        }
        ("DlSheetDlSegmentType", "b32bf6a3-11d2-09f4-6000-f99ac5361ab0") => {
            decoder = super::style::boolean;
            "display_boolean_candidate"
        }
        (
            "DlSheetDlSegmentType" | "DlSheetSmSegmentType",
            "f2fb355d-42d4-07c8-a07b-e085474d8be7",
        ) => {
            decoder = super::style::layer_binding;
            "layer_binding_candidate"
        }
        ("DlSheetDlSegmentType", "b32bf6ac-11d2-09f4-6000-f99ac5361ab0") => {
            decoder = super::style::stroke;
            "stroke_override_candidate"
        }
        ("DlSheetDlSegmentType", "48eb8608-11d2-070c-6000-f99ac5361ab0") => {
            decoder = super::style::color;
            "display_color_candidate"
        }
        ("AppSegmentType", "c1ab98dd-4ed8-5d1f-22c5-51841631b75e") => {
            decoder = super::style::layer;
            "layer_definition_candidate"
        }
        (
            "DlSheetDlSegmentType" | "DlSheetSmSegmentType",
            "a79eacd5-11d1-c281-6000-a38ab46bceb0",
        ) => {
            decoder = super::text::fields;
            "stored_text_candidate"
        }
        (
            "DlSheetDlSegmentType" | "DlSheetSmSegmentType",
            "a79eaccb-11d1-c281-6000-a38ab46bceb0",
        ) => {
            decoder = super::geometry::points;
            "stored_polyline_candidate"
        }
        ("DlSheetDlSegmentType", "d3a55702-11d1-ebbb-62ae-0297584063da")
            if profile.extended_record(type_id) =>
        {
            decoder = super::spline::fields;
            "stored_bspline_candidate"
        }
        ("DlSheetDlSegmentType", "afd5ceeb-11d1-e071-0008-87a406e5dc09")
            if profile.extended_record(type_id) =>
        {
            decoder = super::geometry::ellipse;
            "stored_ellipse_candidate"
        }
        ("DlSheetSmSegmentType", "a79eacd2-11d1-c281-6000-a38ab46bceb0")
            if profile.triangle_flags().is_some() =>
        {
            decoder = super::geometry::triangles;
            "stored_triangles_candidate"
        }
        (
            "DlSheetDlSegmentType" | "DlSheetSmSegmentType",
            "a79eacc7-11d1-c281-6000-a38ab46bceb0",
        ) => {
            decoder = super::geometry::line;
            "stored_line_candidate"
        }
        (
            "DlSheetDlSegmentType" | "DlSheetSmSegmentType",
            "a79eaccd-11d1-c281-6000-a38ab46bceb0",
        ) => {
            decoder = super::geometry::circle;
            "stored_circle_candidate"
        }
        (
            "DlSheetDlSegmentType" | "DlSheetSmSegmentType",
            "a79eaccc-11d1-c281-6000-a38ab46bceb0",
        ) => {
            decoder = super::geometry::arc;
            "stored_arc_candidate"
        }
        (
            "DlSheetDlSegmentType" | "DlSheetSmSegmentType",
            "a79eaccf-11d1-c281-6000-a38ab46bceb0",
        ) => {
            decoder = super::geometry::group;
            "display_group_candidate"
        }
        _ => return Ok(None),
    };
    rse::charge(work, 1)?;
    let mut fields = Fields::new(bytes, source.clone(), work, profile, context.types);
    decoder(&mut fields)?;
    Ok(Some(PayloadObservation {
        record_ordinal: ordinal,
        type_id: type_id.into(),
        source,
        layout: profile.fields_name(),
        proposed_role: role,
        status: "unqualified",
        fields: fields.fields,
    }))
}

#[cfg(feature = "fuzzing")]
pub(super) fn fuzz(bytes: &[u8], limits: &crate::Limits) {
    let mut work = limits.max_records;
    for decoder in [
        super::sheet::document,
        super::sheet::name,
        super::sheet::links,
        super::sheet::space,
        super::sheet::placement,
        super::sheet::border_placement,
        super::sheet::local_display,
        super::sheet::leader_display,
        super::sheet::table_display,
        super::sheet::view_placement,
        super::sheet::view_bitmap,
        super::sheet::point_marker,
        super::style::fonts,
        super::style::attributes,
        super::style::boolean,
        super::style::layer_binding,
        super::style::layer,
        super::style::stroke,
        super::style::color,
        super::sheet::image,
        super::text::fields,
        super::geometry::points,
        super::geometry::triangles,
        super::geometry::group,
        super::geometry::line,
        super::geometry::circle,
        super::geometry::arc,
        super::geometry::ellipse,
        super::spline::fields,
    ] {
        for major in [23, 24, 26, 28, 29, 31] {
            let mut fields = Fields::new(
                bytes,
                SourceSpan::stream("fuzz", "raw", 0, bytes.len()),
                &mut work,
                super::profile::get(major).unwrap(),
                &[[0; 16], [0; 16], POINT3F],
            );
            if decoder(&mut fields).is_ok()
                && fields.fields.iter().any(|f| f.name == "spline_degree")
            {
                let o = PayloadObservation {
                    record_ordinal: 0,
                    type_id: "fuzz".into(),
                    source: fields.source.clone(),
                    layout: "fuzz",
                    proposed_role: "stored_bspline_candidate",
                    status: "unqualified",
                    fields: fields.fields,
                };
                if let Ok(spline) = super::spline::Spline::observation(&o) {
                    for (span, start, end) in spline.spans().take(8) {
                        if rse::charge(&mut work, 3).is_err() {
                            return;
                        }
                        for t in [start, start + (end - start) * 0.5, end] {
                            let _ = spline.evaluate(span, t);
                        }
                    }
                }
            }
        }
    }
    if bytes.len() <= limits.max_property_bytes {
        super::images::fuzz(bytes);
    }
}

#[cfg(test)]
mod marker_tests {
    use super::*;
    #[test]
    fn marker_is_bounded_and_does_not_claim_printed_geometry() {
        let mut bytes = vec![0u8; 26];
        for value in [3.0f64, 11., 0.] {
            bytes.extend(value.to_le_bytes());
        }
        for value in [1.0f32, 0., 0.] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0u16.to_le_bytes());
        let parse = |data: &[u8]| {
            decode(
                "DlSheetSmSegmentType",
                31.into(),
                "41305114-11d2-6450-6000-b4856c2387b0",
                0,
                data,
                SourceSpan::stream("synthetic", "/B", 0, data.len()),
                &mut 100,
            )
        };
        let observation = parse(&bytes).unwrap().unwrap();
        assert_eq!(observation.proposed_role, "stored_point_marker_candidate");
        assert!(matches!(&observation.fields[5].value, FieldValue::F64(v) if v == &[3., 11., 0.]));
        for n in 0..bytes.len() {
            assert!(parse(&bytes[..n]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(parse(&extra).is_err());
        bytes[26..34].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(parse(&bytes).is_err());
    }
}
