//! RSe31/Meta8 framing, independently implemented from the documented layout.
//! cadmpeg's Apache-2.0 reference implementation and CC-BY-4.0 format notes
//! informed this implementation; see THIRD_PARTY_NOTICES.md.
use crate::{read::Reader, Error, Limits, Result};
use std::io::Read;

#[derive(Debug, Clone)]
pub(crate) struct Segment {
    pub name: String,
    pub kind: String,
    pub id: [u8; 16],
    pub major: u8,
    pub start_offset: usize,
    pub end_offset: usize,
}

pub(crate) fn registry(bytes: &[u8]) -> Result<Vec<Segment>> {
    registry_budgeted(bytes, &mut { usize::MAX })
}

pub(crate) fn charge(budget: &mut usize, n: usize) -> Result<()> {
    if n > *budget {
        *budget = 0;
        return Err(Error("RSe aggregate work limit exceeded".into()));
    }
    *budget -= n;
    Ok(())
}

pub(crate) fn registry_budgeted(bytes: &[u8], work: &mut usize) -> Result<Vec<Segment>> {
    let mut r = Reader::new(bytes);
    let n = r.count(65536)?;
    charge(work, n)?;
    let mut entries = Vec::new();
    for _ in 0..n {
        let start_offset = r.pos;
        let name = r.utf16()?;
        let id = r.id()?;
        r.skip(16 + 4)?;
        let objects = r.count(1_000_000)?;
        charge(work, objects)?;
        r.skip(5 * 4 + 4)?;
        let kind = r.utf16()?;
        r.skip(8)?;
        let version = r.take(8)?;
        let major = version[2];
        r.skip(4)?;
        let mut nodes = 1;
        for _ in 0..objects {
            r.skip(16 + 9 + 16 + 4)?;
            nodes = r.count(1_000_000)?;
        }
        let count = nodes
            .checked_sub(1)
            .ok_or_else(|| Error("zero registry node count".into()))?;
        charge(work, count)?;
        r.skip(
            count
                .checked_mul(22)
                .ok_or_else(|| Error("node length overflow".into()))?,
        )?;
        entries.push(Segment {
            name,
            kind,
            id,
            major,
            start_offset,
            end_offset: r.pos,
        });
    }
    r.skip(4)?;
    for _ in 0..2 {
        let n = r.count(1_000_000)?;
        charge(work, n)?;
        r.skip(n * 16)?;
    }
    r.finish()?;
    Ok(entries)
}

/// Exactly one compressed member, bounded both while streaming and before return.
pub(crate) fn inflate(data: &[u8], limit: usize) -> Result<(Vec<u8>, &'static str)> {
    inflate_budgeted(data, limit, &mut { limit })
}

/// Charge emitted bytes even when decompression or later framing fails. This
/// prevents many corrupt members from repeatedly spending the same allowance.
pub(crate) fn inflate_budgeted(
    data: &[u8],
    limit: usize,
    remaining: &mut usize,
) -> Result<(Vec<u8>, &'static str)> {
    let limit = limit.min(*remaining);
    let mut out = Vec::new();
    let mut chunk = [0; 16 * 1024];
    let codec = if data.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        let used = zstd::zstd_safe::find_frame_compressed_size(data)
            .map_err(|e| Error(format!("zstd frame: {e}")))?;
        if used != data.len() {
            return Err(Error("trailing data after zstd frame".into()));
        }
        let mut decoder =
            zstd::stream::read::Decoder::new(data).map_err(|e| Error(e.to_string()))?;
        decoder
            .window_log_max(26)
            .map_err(|e| Error(e.to_string()))?;
        loop {
            let cap = chunk
                .len()
                .min(limit.saturating_sub(out.len()).saturating_add(1));
            let n = decoder.read(&mut chunk[..cap]).map_err(|e| {
                // A Read error need not disclose bytes written before failure.
                // Exhaust the allowance rather than retry an uncharged decode.
                *remaining = 0;
                Error(e.to_string())
            })?;
            if n == 0 {
                break;
            }
            *remaining = remaining.saturating_sub(n);
            if n > limit.saturating_sub(out.len()) {
                return Err(Error("decompression byte limit exceeded".into()));
            }
            out.extend_from_slice(&chunk[..n]);
        }
        "zstd"
    } else {
        let mut decoder = flate2::Decompress::new(true);
        loop {
            let before_in = decoder.total_in();
            let before_out = decoder.total_out();
            let cap = chunk
                .len()
                .min(limit.saturating_sub(out.len()).saturating_add(1));
            let result = decoder.decompress(
                &data[before_in as usize..],
                &mut chunk[..cap],
                flate2::FlushDecompress::None,
            );
            let produced = (decoder.total_out() - before_out) as usize;
            *remaining = remaining.saturating_sub(produced);
            let status = result.map_err(|e| Error(e.to_string()))?;
            if produced > limit.saturating_sub(out.len()) {
                return Err(Error("decompression byte limit exceeded".into()));
            }
            out.extend_from_slice(&chunk[..produced]);
            if status == flate2::Status::StreamEnd {
                if decoder.total_in() as usize != data.len() {
                    return Err(Error("trailing data after zlib member".into()));
                }
                break;
            }
            if decoder.total_in() == before_in && produced == 0 {
                return Err(Error("incomplete zlib member".into()));
            }
        }
        "zlib"
    };
    Ok((out, codec))
}

pub(crate) struct Meta {
    pub id: [u8; 16],
    pub name: String,
    pub blocks: Vec<u32>,
    pub types: Vec<[u8; 16]>,
    pub inflated_bytes: usize,
    pub compressed_offset: usize,
    pub codec: &'static str,
    pub state_words: [u32; 3],
    pub block_table_offset: usize,
    pub type_table_offset: usize,
    pub reference_sections: Vec<MetaSection>,
}
pub(crate) struct MetaSection {
    pub number: u8,
    pub count: usize,
    pub offset: usize,
    pub bytes: Vec<u8>,
}

/// Bounded layouts selected by the caller's segment profile, never by retrying
/// a failed parse. Extra descriptors do not widen the low-byte record selector.
#[derive(Clone, Copy)]
pub(crate) struct MetaLayout {
    pub type_limit: usize,
    pub section9_entry_bytes: usize,
}
impl MetaLayout {
    pub const STANDARD: Self = Self {
        type_limit: 256,
        section9_entry_bytes: 19,
    };
    pub const DRAWING_DOC_DC: Self = Self {
        type_limit: 4096,
        section9_entry_bytes: 15,
    };
    pub const DRAWING_SHEET_DC: Self = Self {
        type_limit: 256,
        section9_entry_bytes: 15,
    };
}
pub(crate) fn meta_identity(bytes: &[u8]) -> Result<([u8; 16], String)> {
    let mut r = Reader::new(bytes);
    if r.text()? != "RSe Meta Stream Version 8" || r.u16()? != 8 {
        return Err(Error("unsupported RSe meta profile".into()));
    }
    r.skip(16)?;
    let name = r.utf16()?;
    Ok((r.id()?, name))
}
pub(crate) fn meta(bytes: &[u8], limits: &Limits) -> Result<Meta> {
    meta_budgeted(bytes, limits, &mut { limits.max_inflated_bytes }, &mut {
        usize::MAX
    })
}

pub(crate) fn meta_budgeted(
    bytes: &[u8],
    limits: &Limits,
    expanded: &mut usize,
    work: &mut usize,
) -> Result<Meta> {
    meta_layout_budgeted(bytes, limits, expanded, work, MetaLayout::STANDARD)
}

pub(crate) fn meta_layout_budgeted(
    bytes: &[u8],
    limits: &Limits,
    expanded: &mut usize,
    work: &mut usize,
    layout: MetaLayout,
) -> Result<Meta> {
    let mut r = Reader::new(bytes);
    if r.text()? != "RSe Meta Stream Version 8" || r.u16()? != 8 {
        return Err(Error("unsupported RSe meta profile".into()));
    }
    r.skip(16)?;
    let name = r.utf16()?;
    let id = r.id()?;
    let state_words = [r.u32()?, r.u32()?, r.u32()?];
    r.text()?;
    r.text()?;
    r.u8()?;
    let compressed_offset = r.pos;
    let (body, codec) = inflate_budgeted(&bytes[r.pos..], limits.max_inflated_bytes, expanded)?;
    let mut r = Reader::new(&body);
    r.skip(14)?;
    let mut blocks = Vec::new();
    let mut types = Vec::new();
    let mut type_footer = 0;
    let mut block_table_offset = 0;
    let mut type_table_offset = 0;
    for (section, size) in [(1, 4), (2, 10), (3, 28), (4, 28)] {
        let n = r.count(limits.max_records)?;
        charge(work, n)?;
        if section == 4 && n > layout.type_limit {
            return Err(Error(format!(
                "RSe type table exceeds {}",
                layout.type_limit
            )));
        }
        let offset = r.pos;
        let payload = r.take(n * size)?;
        if section == 1 {
            block_table_offset = offset;
            blocks = payload
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
        }
        if section == 4 {
            type_table_offset = offset;
            types = payload
                .chunks_exact(28)
                .map(|b| b[..16].try_into().unwrap())
                .collect();
            type_footer = r.pos;
        }
        if r.u32()? as usize != 4 + n * size {
            return Err(Error("RSe forward section span mismatch".into()));
        }
    }
    let mut end = body
        .len()
        .checked_sub(16)
        .ok_or_else(|| Error("truncated meta terminal id".into()))?;
    let mut payload_len = 72;
    let mut reference_sections = Vec::new();
    for number in (5..=11).rev() {
        let start = end
            .checked_sub(payload_len + 8)
            .ok_or_else(|| Error("meta backward span underflow".into()))?;
        let mut tail = Reader::new(&body[start..end]);
        let previous = tail.u32()? as usize;
        let count = tail.u32()? as usize;
        if number != 5 && count > limits.max_records {
            return Err(Error(format!("meta section {number} count limit exceeded")));
        }
        if number != 5 {
            charge(work, count)?;
        }
        if previous < 4 {
            return Err(Error("invalid meta backward span".into()));
        }
        let size = match number {
            8 => Some(20),
            9 => Some(layout.section9_entry_bytes),
            10 => Some(8),
            11 => Some(4),
            _ => None,
        };
        if let Some(size) = size {
            if count.checked_mul(size) != Some(payload_len) {
                return Err(Error("meta backward section length mismatch".into()));
            }
        }
        if matches!(number, 7 | 8 | 10) {
            reference_sections.push(MetaSection {
                number,
                count,
                offset: start + 8,
                bytes: body[start + 8..end].to_vec(),
            });
        }
        end = start;
        payload_len = previous - 4;
    }
    if end != type_footer {
        return Err(Error("meta section chain does not join type footer".into()));
    }
    Ok(Meta {
        id,
        name,
        blocks,
        types,
        inflated_bytes: body.len(),
        compressed_offset,
        codec,
        state_words,
        block_table_offset,
        type_table_offset,
        reference_sections,
    })
}

pub(crate) struct NamedReference {
    pub name: String,
    pub value: u32,
    pub start: usize,
    pub end: usize,
}

fn extended_trailer(
    r: &mut Reader<'_>,
    work: &mut usize,
    keep: bool,
) -> Result<Vec<NamedReference>> {
    let mut references = Vec::new();
    if r.u8()? == 0 {
        return Ok(references);
    }
    let count = r.u32()?;
    if count & 0x8000_0000 != 0 {
        return Ok(references);
    }
    if count > 65536 {
        return Err(Error("trailer property limit".into()));
    }
    charge(work, count as usize)?;
    for _ in 0..count {
        r.text()?;
        match r.u32()? {
            1 => r.skip(3)?,
            3 | 7 => r.skip(4)?,
            8 | 10 => r.skip(6)?,
            11 => r.skip(10)?,
            14 => {
                r.skip(2)?;
                let n = r.count(64 * 1024 * 1024)?;
                r.skip(n)?;
            }
            tag => return Err(Error(format!("unsupported trailer property type {tag}"))),
        }
    }
    if r.u16()? != 6 || r.u16()? != 0x3000 {
        return Err(Error("invalid trailer reference marker".into()));
    }
    let count = r.count(65536)?;
    charge(work, count)?;
    if count > 0 {
        r.skip(8)?;
        for _ in 0..count {
            let start = r.pos;
            let name = r.text()?;
            let value = r.u32()?;
            if keep {
                references.push(NamedReference {
                    name,
                    value,
                    start,
                    end: r.pos,
                });
            }
        }
    }
    Ok(references)
}

pub(crate) struct Record {
    pub ordinal: usize,
    pub kind: [u8; 16],
    pub selector: u32,
    pub start: usize,
    pub end: usize,
    pub frame_end: usize,
    pub references: Vec<NamedReference>,
}

pub(crate) struct RecordTable {
    pub records: Vec<Record>,
    pub terminal_offset: usize,
    pub opaque_start: usize,
}

pub(crate) fn records(data: &[u8], meta: &Meta, major: u8) -> Result<Vec<Record>> {
    Ok(record_table(data, meta, major, &mut { usize::MAX }, false)?.records)
}

pub(crate) fn record_table(
    data: &[u8],
    meta: &Meta,
    major: u8,
    work: &mut usize,
    keep_references: bool,
) -> Result<RecordTable> {
    charge(work, meta.blocks.len())?;
    let mut r = Reader::new(data);
    let mut records = Vec::new();
    for (ordinal, block) in meta.blocks.iter().enumerate() {
        if block & 0x8000_0000 == 0 {
            continue;
        }
        let selector = r.u32()?;
        let kind = *meta
            .types
            .get(selector as u8 as usize)
            .ok_or_else(|| Error("absent RSe type index".into()))?;
        let len = (block & 0x7fff_ffff) as usize;
        let start = r.pos;
        r.skip(len)?;
        let end = r.pos;
        let trailing = r.u32()? as usize;
        if trailing != 0 && trailing != len {
            return Err(Error("RSe trailing record length mismatch".into()));
        }
        let references = if major > 18 {
            extended_trailer(&mut r, work, keep_references)?
        } else {
            Vec::new()
        };
        records.push(Record {
            ordinal,
            kind,
            selector,
            start,
            end,
            frame_end: r.pos,
            references,
        });
    }
    let terminal_offset = r.pos;
    if r.u32()? != u32::MAX {
        return Err(Error("missing RSe bulk terminal marker".into()));
    }
    Ok(RecordTable {
        records,
        terminal_offset,
        opaque_start: r.pos,
    })
}
