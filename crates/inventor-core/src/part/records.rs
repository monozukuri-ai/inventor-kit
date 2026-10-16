//! Observed major31 layouts. Field boundaries do not establish write semantics.
use super::*;
use acis_core::sab::{parse_sab, SabLimits};

const BULK_PREFIX: [u8; 18] = [
    0x9e, 0xc2, 0x2b, 0xa4, 0xd4, 0x11, 0xe8, 0x01, 0x60, 0x00, 0x2d, 0xb3, 0xee, 0x29, 0xfb, 0xb0,
    0x04, 0x02,
];
/// Observed stream-name encoding, not a substitute for Meta/registry ownership.
pub(super) fn observed_stream_token(id: &[u8; 16]) -> String {
    let alphabet = b"abcdefghijklmnopqrstuvwxyz123456";
    let words: Vec<_> = id
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let mut token = String::with_capacity(26);
    for word in &words {
        for shift in (0..6).rev() {
            token.push(alphabet[((word >> (shift * 5)) & 31) as usize] as char);
        }
    }
    for pair in words.chunks_exact(2) {
        token.push(alphabet[(((pair[0] >> 30) << 2) | (pair[1] >> 30)) as usize] as char);
    }
    token
}
pub(super) fn check_bulk_envelope(bytes: &[u8]) -> Result<()> {
    if bytes.get(..18) != Some(BULK_PREFIX.as_slice())
        || bytes.get(18..22) != Some([0x28, 0xb5, 0x2f, 0xfd].as_slice())
    {
        return Err(Error("major31 B envelope/codec mismatch".into()));
    }
    Ok(())
}
struct Observer<'a> {
    reader: Reader<'a>,
    source: &'a SourceSpan,
    fields: Vec<WireField>,
    references: Vec<(String, u32, SourceSpan)>,
}
impl<'a> Observer<'a> {
    fn new(bytes: &'a [u8], source: &'a SourceSpan) -> Self {
        Self {
            reader: Reader::new(bytes),
            source,
            fields: vec![],
            references: vec![],
        }
    }
    fn source(&self, start: usize) -> SourceSpan {
        let mut source = self.source.clone();
        source.start_offset += start;
        source.end_offset = self.source.start_offset + self.reader.pos;
        source
    }
    fn word(&mut self, name: &str, reference: bool) -> Result<u32> {
        let start = self.reader.pos;
        let value = self.reader.u32()?;
        let source = self.source(start);
        self.fields
            .push(field(name, WireValue::U32(value), source.clone()));
        if reference {
            self.references.push((name.into(), value, source));
        }
        Ok(value)
    }
    fn small(&mut self, name: &str) -> Result<u16> {
        let start = self.reader.pos;
        let value = self.reader.u16()?;
        self.fields
            .push(field(name, WireValue::U16(value), self.source(start)));
        Ok(value)
    }
    fn byte(&mut self, name: &str) -> Result<u8> {
        let start = self.reader.pos;
        let value = self.reader.u8()?;
        self.fields
            .push(field(name, WireValue::U8(value), self.source(start)));
        Ok(value)
    }
    fn wide(&mut self, name: &str) -> Result<()> {
        let start = self.reader.pos;
        let value = self.reader.utf16()?;
        self.fields
            .push(field(name, WireValue::Utf16(value), self.source(start)));
        Ok(())
    }
    fn identity(&mut self, name: &str) -> Result<()> {
        let start = self.reader.pos;
        let value = guid(&self.reader.id()?);
        self.fields
            .push(field(name, WireValue::Guid(value), self.source(start)));
        Ok(())
    }
    fn number(&mut self, name: &str) -> Result<()> {
        let start = self.reader.pos;
        let value = f64::from_bits(self.reader.u64()?);
        if !value.is_finite() {
            return Err(Error("nonfinite part record number".into()));
        }
        self.fields
            .push(field(name, WireValue::F64(value), self.source(start)));
        Ok(())
    }
    fn header(&mut self, linked: bool) -> Result<()> {
        self.word("header_state", false)?;
        self.small("header_id")?;
        self.word("next_reference", true)?;
        self.word("header_flags", false)?;
        self.word("header_word_14", false)?;
        self.word("context_reference", true)?;
        self.word(
            if linked {
                "parent_reference"
            } else {
                "header_word_22"
            },
            linked,
        )?;
        if linked {
            self.word("sibling_reference", true)?;
        }
        Ok(())
    }
    fn list(&mut self, name: &str) -> Result<()> {
        if self.word(&format!("{name}_marker"), false)? != 0x3000_0002 {
            return Err(Error("unsupported part reference list marker".into()));
        }
        let count = self.word(&format!("{name}_count"), false)? as usize;
        if count > 4096 {
            return Err(Error("part record reference list limit exceeded".into()));
        }
        if count > 0 {
            self.word(&format!("{name}_word_0"), false)?;
            self.word(&format!("{name}_word_1"), false)?;
            for index in 0..count {
                self.word(&format!("{name}_{index}"), true)?;
            }
        }
        Ok(())
    }
}

fn decode<'a>(
    record: &'a PartRecord,
    kind: &str,
    limits: &Limits,
    work: &mut usize,
) -> Result<(Observer<'a>, Option<KernelObservation>)> {
    let mut o = Observer::new(&record.payload.0, &record.source);
    let mut kernel = None;
    match (kind, record.type_id.as_str()) {
        ("PmDcSegmentType", "90874d16-11d0-d1f8-0008-cabc0663dc09") => {
            o.word("header_state", false)?;
            o.small("header_id")?;
            o.word("next_reference", true)?;
            o.word("header_flags", false)?;
            o.word("header_word_14", false)?;
            o.identity("document_class")?;
            o.wide("document_name")?;
            o.list("document_reference")?;
            for i in 0..3 {
                o.word(&format!("document_word_{i}"), false)?;
            }
            o.word("root_reference", true)?;
            o.word("unit_record_reference", true)?;
            o.word("document_word_3", false)?;
            o.word("document_word_4", false)?;
            o.word("application_reference", true)?;
            o.word("document_word_5", false)?;
            o.word("root_reference_repeat", true)?;
            o.word("end_reference", true)?;
            o.word("body_binding_reference", true)?;
            o.word("document_word_6", false)?;
            o.word("document_helper_reference", true)?;
            o.word("body_root_reference", true)?;
            o.wide("result_segment_name")?;
            o.number("document_scale_word")?;
            o.word("document_tail_word", false)?;
        }
        ("PmDcSegmentType", "90874d63-11d0-d1f8-0008-cabc0663dc09") => {
            o.word("header_state", false)?;
            o.small("header_id")?;
            o.word("next_reference", true)?;
            o.word("header_flags", false)?;
            o.word("header_word_14", false)?;
            o.word("context_reference", true)?;
            o.list("member_reference")?;
            let start = o.reader.pos;
            let value = Binary(o.reader.take(o.reader.bytes.len() - start)?.to_vec());
            o.fields.push(field(
                "unknown_suffix",
                WireValue::Bytes(value),
                o.source(start),
            ));
        }
        ("PmDcSegmentType", "90874d91-11d0-d1f8-0008-cabc0663dc09") => {
            o.header(false)?;
            o.word("feature_state_word", false)?;
            o.word("feature_word_30", false)?;
            o.list("property_reference")?;
            o.word("feature_tail_word", false)?;
        }
        ("PmDcSegmentType", "90874d47-11d0-d1f8-0008-cabc0663dc09") => {
            o.header(false)?;
            o.word("body_word_26", false)?;
            let state = o.word("body_word_30", false)?;
            o.word("body_word_34", state == 0xc20)?;
        }
        ("PmDcSegmentType", "8f41fd24-11d2-6eac-0008-2aab32a3dc09") => {
            o.header(false)?;
            o.word("end_word", false)?;
        }
        ("PmDcSegmentType", "90874d28-11d0-d1f8-0008-cabc0663dc09") => {
            o.header(false)?;
            o.word("property_word_26", false)?;
            o.word("property_word_30", false)?;
            o.number("property_number")?;
            if o.byte("property_bool")? > 1 {
                return Err(Error("invalid part property Boolean".into()));
            }
        }
        ("PmDcSegmentType", "ae5e4082-4ecb-953c-684d-5ca55fb19926") => {
            o.header(false)?;
            o.word("label_reference", true)?;
        }
        ("PmDcSegmentType", "56970dfa-11d4-2cdf-c000-9b9645df724f") => {
            o.header(false)?;
            o.word("feature_label_word_26", false)?;
            o.word("feature_label_word_30", false)?;
            o.byte("feature_label_byte_34")?;
        }
        ("PmDcSegmentType", "2b48a42b-11d2-6458-6000-74b79b49ebb0") => {
            o.header(true)?;
            o.word("label_index", false)?;
            o.list("participant_reference")?;
            let start = o.reader.pos;
            let value = o.reader.utf16()?;
            o.fields
                .push(field("label", WireValue::Utf16(value), o.source(start)));
            let start = o.reader.pos;
            let value = guid(&o.reader.id()?);
            o.fields.push(field(
                "label_class",
                WireValue::Guid(value),
                o.source(start),
            ));
        }
        ("PmDcSegmentType" | "PmBrepSegmentType", "cc0f7521-11d1-8027-e386-19962259017a") => {
            o.word("header_state", false)?;
            o.small("header_id")?;
            o.word("geometry_word_6", false)?;
            o.byte("geometry_byte_10")?;
            o.word("kernel_reference", true)?;
            o.word("geometry_word_15", false)?;
            if o.word("schema", false)? != 44 {
                return Err(Error("part geometry record schema mismatch".into()));
            }
        }
        ("PmDcSegmentType" | "PmBrepSegmentType", "f645595c-11d5-1333-1000-60a6bba647b5") => {
            o.word("header_state", false)?;
            o.small("header_id")?;
            o.word("header_word_6", false)?;
            if o.word("schema", false)? != 44 {
                return Err(Error("part carrier record schema mismatch".into()));
            }
            let end = record
                .payload
                .0
                .len()
                .checked_sub(18)
                .filter(|&end| end >= 14)
                .ok_or_else(|| Error("truncated part kernel carrier".into()))?;
            let bytes = &record.payload.0[14..end];
            // Exact typed offset; a Result record sharing this GUID is not a SAB carrier.
            let parsed = parse_sab(
                bytes,
                &record.source.source_id,
                &SabLimits {
                    max_bytes: limits.max_inflated_bytes,
                    max_records: *work,
                    ..Default::default()
                },
            )
            .map_err(|e| Error(e.to_string()))?;
            rse::charge(work, parsed.model.entities().len())?;
            if !matches!(parsed.header.save_version, 23100 | 23200)
                || parsed.header.integer_width != 4
                || parsed.header.scale != 10.0
            {
                return Err(Error("part source kernel profile mismatch".into()));
            }
            let mut source = record.source.clone();
            source.start_offset += 14;
            source.end_offset = record.source.start_offset + end;
            let active_end = parsed
                .history
                .as_ref()
                .map(|s| s.start_offset)
                .unwrap_or(bytes.len());
            let active_table_sha256 = digest(&bytes[parsed.header.data_offset..active_end]);
            let mut active_table_source = source.clone();
            active_table_source.start_offset += parsed.header.data_offset;
            active_table_source.end_offset = source.start_offset + active_end;
            let history = parsed.history.map(|range| {
                let mut history = source.clone();
                history.start_offset = source.start_offset + range.start_offset;
                history.end_offset = source.start_offset + range.end_offset;
                history
            });
            let mut type_counts = BTreeMap::new();
            let mut bodies = Vec::new();
            let mut custom_attributes = Vec::new();
            let mut entity_identifiers = Vec::new();
            for entity in parsed.model.entities() {
                let raw = entity.raw();
                *type_counts.entry(raw.type_name.clone()).or_default() += 1;
                if let Some(range) = &raw.source {
                    let mut entity_source = source.clone();
                    entity_source.start_offset = source.start_offset + range.start_offset;
                    entity_source.end_offset = source.start_offset + range.end_offset;
                    entity_identifiers.push(KernelEntityIdentity {
                        ordinal: raw.index,
                        type_name: raw.type_name.clone(),
                        entity_id: raw.entity_id.as_ref().map(ToString::to_string),
                        source: entity_source,
                    });
                }
                if raw.type_name == "body" {
                    bodies.push(KernelBody {
                        ordinal: raw.index,
                        entity_id: raw.entity_id.as_ref().map(ToString::to_string),
                    });
                }
                if raw.type_name == "ATTRIB_CUSTOM-attrib" {
                    if let (Some(range), Some(bytes)) = (&raw.source, &raw.raw_data) {
                        let mut attribute_source = source.clone();
                        attribute_source.start_offset = source.start_offset + range.start_offset;
                        attribute_source.end_offset = source.start_offset + range.end_offset;
                        custom_attributes.push(KernelAttribute {
                            ordinal: raw.index,
                            entity_id: raw.entity_id.as_ref().map(ToString::to_string),
                            source: attribute_source,
                            bytes: Binary(bytes.clone()),
                            strings: raw
                                .values
                                .iter()
                                .filter_map(|v| match v {
                                    acis_core::AcisValue::String(s) => Some(s.clone()),
                                    acis_core::AcisValue::CountedString(s) => Some(s.value.clone()),
                                    _ => None,
                                })
                                .collect(),
                        });
                    }
                }
            }
            let h = parsed.header;
            kernel = Some(KernelObservation {
                sha256: digest(bytes),
                source,
                save_version: h.save_version,
                integer_width: h.integer_width,
                scale: h.scale,
                resabs: h.resabs,
                resnor: h.resnor,
                product_id: h.product_id,
                modeler_version: h.modeler_version,
                creation_date: h.creation_date,
                active_table_source,
                active_table_sha256,
                type_counts,
                bodies,
                custom_attributes,
                entity_identifiers,
                history,
            });
            o.reader.pos = end;
            o.word("carrier_self_reference", true)?;
            if o.byte("carrier_enabled")? > 1 {
                return Err(Error("invalid part carrier Boolean".into()));
            }
            o.word("delta_state", false)?;
            if o.byte("carrier_padding")? != 0 {
                return Err(Error("invalid part carrier padding".into()));
            }
            o.word("history_reference", true)?;
            if o.word("carrier_terminal", false)? != u32::MAX {
                return Err(Error("invalid part carrier terminal".into()));
            }
        }
        _ => return Err(Error("no observed typed part record layout".into())),
    }
    o.reader.finish()?;
    Ok((o, kernel))
}

pub(super) fn observe(segment: &mut PartSegment, limits: &Limits, work: &mut usize) -> Result<()> {
    let targets: BTreeMap<usize, String> = segment
        .records
        .iter()
        .map(|r| (r.ordinal, r.type_id.clone()))
        .collect();
    for record in &mut segment.records {
        match decode(record, &segment.kind, limits, work) {
            Ok((o, kernel)) => {
                let fields = o.fields;
                let references = o
                    .references
                    .into_iter()
                    .map(|(field, raw, source)| {
                        let index = (raw & 0x7fff_ffff) as usize;
                        let target = index.checked_sub(1).filter(|&i| targets.contains_key(&i));
                        RecordReference {
                            field,
                            raw,
                            qualifier_bit: raw >> 31 != 0,
                            target_ordinal: target,
                            target_type_id: target.map(|i| targets[&i].clone()),
                            status: if index == 0 {
                                "null"
                            } else if raw == u32::MAX {
                                "sentinel"
                            } else if target.is_some() {
                                "stored_record"
                            } else {
                                "unresolved"
                            },
                            source,
                        }
                    })
                    .collect();
                record.fields = fields;
                record.references = references;
                record.kernel = kernel;
                record.observation_status =
                    if record.fields.iter().any(|f| f.name == "unknown_suffix") {
                        "prefix_observed"
                    } else {
                        "observed_layout"
                    };
            }
            Err(error) => {
                if error.0 != "no observed typed part record layout" {
                    record.observation_status = "unavailable";
                    record.diagnostics.push(warning(
                        "part.record_observation_unavailable",
                        error.0,
                        Some(record.source.clone()),
                    ));
                }
            }
        }
        if *work == 0 {
            return Err(Error("part observation work limit exhausted".into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_reference_is_unaligned_and_schema_and_suffix_are_exact() {
        let mut p = 0u32.to_le_bytes().to_vec();
        p.extend(8u16.to_le_bytes());
        p.extend(2u32.to_le_bytes());
        p.push(0);
        p.extend(0x8000_0005u32.to_le_bytes());
        p.extend(19u32.to_le_bytes());
        p.extend(44u32.to_le_bytes());
        let r = record("cc0f7521-11d1-8027-e386-19962259017a", p.clone());
        let (o, _) = decode(&r, "PmBrepSegmentType", &Limits::default(), &mut 100).unwrap();
        assert_eq!(o.references[0].1, 0x8000_0005);
        assert_eq!(o.references[0].2.start_offset, 111);
        for end in 0..p.len() {
            let cut = record(&r.type_id, p[..end].to_vec());
            assert!(decode(&cut, "PmBrepSegmentType", &Limits::default(), &mut 100).is_err());
        }
        let mut future = p.clone();
        future[19..23].copy_from_slice(&45u32.to_le_bytes());
        assert!(decode(
            &record(&r.type_id, future),
            "PmBrepSegmentType",
            &Limits::default(),
            &mut 100
        )
        .is_err());
        p.push(0);
        assert!(decode(
            &record(&r.type_id, p),
            "PmBrepSegmentType",
            &Limits::default(),
            &mut 100
        )
        .is_err());
    }
    fn record(type_id: &str, payload: Vec<u8>) -> PartRecord {
        PartRecord {
            ordinal: 0,
            selector_word: 0,
            type_index: 0,
            type_id: type_id.into(),
            block_word: 0x8000_0000 | payload.len() as u32,
            source: span("test", "/RSeStorage/Btest", 100, 100 + payload.len(), true),
            frame_source: span("test", "/RSeStorage/Btest", 96, 108 + payload.len(), true),
            payload_sha256: digest(&payload),
            payload: Binary(payload),
            trailer: Binary(vec![]),
            named_references: vec![],
            observation_status: "unsupported",
            fields: vec![],
            references: vec![],
            kernel: None,
            diagnostics: vec![],
        }
    }
    #[test]
    fn observed_label_is_exact_unicode_and_not_a_signature_scan() {
        let mut payload = vec![];
        payload.extend(0u32.to_le_bytes());
        payload.extend(40u16.to_le_bytes());
        for word in [0u32, 0, 0, 0x8000_000c, 0, 0x8000_0028, 11, 0x3000_0002, 0] {
            payload.extend(word.to_le_bytes());
        }
        let name: Vec<_> = "M1 名称".encode_utf16().collect();
        payload.extend((name.len() as u32).to_le_bytes());
        for word in name {
            payload.extend(word.to_le_bytes());
        }
        payload.extend([0; 16]);
        let r = record("2b48a42b-11d2-6458-6000-74b79b49ebb0", payload.clone());
        let (o, kernel) = decode(&r, "PmDcSegmentType", &Limits::default(), &mut 1000).unwrap();
        assert!(kernel.is_none());
        assert!(
            matches!(&o.fields.iter().find(|f| f.name == "label").unwrap().value,
            WireValue::Utf16(name) if name == "M1 名称")
        );
        assert_eq!(
            o.references
                .iter()
                .find(|(n, _, _)| n == "context_reference")
                .unwrap()
                .2
                .start_offset,
            118
        );
        assert!(decode(&r, "PmAppSegmentType", &Limits::default(), &mut 1000).is_err());
        for end in 0..payload.len() {
            let truncated = record(&r.type_id, payload[..end].to_vec());
            assert!(decode(&truncated, "PmDcSegmentType", &Limits::default(), &mut 1000).is_err());
        }
        payload.push(0);
        assert!(decode(
            &record(&r.type_id, payload),
            "PmDcSegmentType",
            &Limits::default(),
            &mut 1000
        )
        .is_err());
    }
    #[test]
    fn same_type_guid_in_result_never_promotes_a_kernel_carrier() {
        let r = record("f645595c-11d5-1333-1000-60a6bba647b5", vec![0; 32]);
        let e = decode(&r, "PmResultSegmentType", &Limits::default(), &mut 1000)
            .err()
            .unwrap();
        assert_eq!(e.0, "no observed typed part record layout");
        assert!(decode(&r, "PmDcSegmentType", &Limits::default(), &mut 1000).is_err());
    }
    #[test]
    fn native_token_vector_keeps_all_four_words_and_high_bits() {
        let id = [
            0x1b, 0x7e, 0x6c, 0x8e, 0xc0, 0x73, 0xa5, 0x43, 0x91, 0x0b, 0x37, 0x63, 0x70, 0xd9,
            0xff, 0x3a,
        ];
        assert_eq!(observed_stream_token(&id), "hgy6q2b1k35artoc3r4p6wlqje");
        assert_eq!(
            observed_stream_token(&[0; 16]),
            "aaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            observed_stream_token(&[255; 16]),
            "666666666666666666666666pp"
        );
    }
    #[test]
    fn unknown_bulk_prefix_and_codec_are_rejected() {
        let mut b = BULK_PREFIX.to_vec();
        b.extend([0x28, 0xb5, 0x2f, 0xfd]);
        assert!(check_bulk_envelope(&b).is_ok());
        for index in [0, 16, 18] {
            let mut invalid = b.clone();
            invalid[index] ^= 1;
            assert!(check_bulk_envelope(&invalid).is_err());
        }
        for end in 0..b.len() {
            assert!(check_bulk_envelope(&b[..end]).is_err());
        }
    }
    #[test]
    fn counted_references_keep_qualified_words_and_source_offsets() {
        let words = [0x3000_0002u32, 2, 3, 16, 0x8000_000c, 0];
        let b: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let source = span("sample", "/RSeStorage/Btoken", 100, 124, true);
        let mut o = Observer::new(&b, &source);
        o.list("slot").unwrap();
        o.reader.finish().unwrap();
        assert_eq!(o.references[0].1, 0x8000_000c);
        assert_eq!(o.references[0].2.start_offset, 116);
        assert_eq!(o.references[1].1, 0);
        for end in 0..b.len() {
            assert!(Observer::new(&b[..end], &source).list("slot").is_err());
        }
        let mut invalid = b.clone();
        invalid[4..8].copy_from_slice(&4097u32.to_le_bytes());
        assert!(Observer::new(&invalid, &source).list("slot").is_err());
    }
}
