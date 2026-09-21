//! Typed wire observations. Roles are hypotheses, never a qualified display list.
use super::{FieldObservation, FieldValue, PayloadObservation};
use crate::{document::SourceSpan, read::Reader, rse, Error, Result};

pub(super) struct Fields<'a, 'b> {
    pub r: Reader<'a>,
    pub work: &'b mut usize,
    source: SourceSpan,
    pub fields: Vec<FieldObservation>,
}
impl<'a, 'b> Fields<'a, 'b> {
    fn new(bytes: &'a [u8], source: SourceSpan, work: &'b mut usize) -> Self {
        Self {
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
        self.require(0x30000002)?;
        let count = self.r.count(65536)?;
        rse::charge(self.work, count)?;
        if count != 0 {
            if self.r.u32()? < count as u32 {
                return Err(Error("drawing list capacity below count".into()));
            }
            self.require(0x10)?;
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
    type_id: &str,
    ordinal: usize,
    bytes: &[u8],
    source: SourceSpan,
    work: &mut usize,
) -> Result<Option<PayloadObservation>> {
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
        ("DlSheetSmSegmentType", "8a6d1381-11d1-6b56-6000-38bd861c3cb0") => {
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
    let mut fields = Fields::new(bytes, source.clone(), work);
    decoder(&mut fields)?;
    Ok(Some(PayloadObservation {
        record_ordinal: ordinal,
        type_id: type_id.into(),
        source,
        layout: "idw-major31-typed-fields-v1",
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
        super::geometry::group,
        super::geometry::line,
        super::geometry::circle,
        super::geometry::arc,
    ] {
        let mut fields = Fields::new(
            bytes,
            SourceSpan::stream("fuzz", "raw", 0, bytes.len()),
            &mut work,
        );
        let _ = decoder(&mut fields);
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
