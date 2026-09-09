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
    let mut r = Reader::new(bytes);
    let n = r.count(65536)?;
    let mut entries = Vec::new();
    for _ in 0..n {
        let start_offset = r.pos;
        let name = r.utf16()?;
        let id = r.id()?;
        r.skip(16 + 4)?;
        let objects = r.count(1_000_000)?;
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
        r.skip(n * 16)?;
    }
    r.finish()?;
    Ok(entries)
}

/// Exactly one compressed member, bounded both while streaming and before return.
pub(crate) fn inflate(data: &[u8], limit: usize) -> Result<(Vec<u8>, &'static str)> {
    if data.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
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
        let mut out = Vec::new();
        decoder
            .take(limit as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| Error(e.to_string()))?;
        if out.len() > limit {
            return Err(Error("decompression byte limit exceeded".into()));
        }
        Ok((out, "zstd"))
    } else {
        // A single streaming state machine both enforces the output limit and
        // proves StreamEnd. The high-level reader required a second complete
        // decompression (and output allocation) to reject truncated members.
        let mut decoder = flate2::Decompress::new(true);
        let mut out = Vec::new();
        let mut chunk = [0; 16 * 1024];
        loop {
            let before_in = decoder.total_in();
            let before_out = decoder.total_out();
            let cap = chunk
                .len()
                .min(limit.saturating_sub(out.len()).saturating_add(1));
            let status = decoder
                .decompress(
                    &data[before_in as usize..],
                    &mut chunk[..cap],
                    flate2::FlushDecompress::None,
                )
                .map_err(|e| Error(e.to_string()))?;
            let produced = (decoder.total_out() - before_out) as usize;
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
        Ok((out, "zlib"))
    }
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
    let (body, codec) = inflate(&bytes[r.pos..], limits.max_inflated_bytes)?;
    let mut r = Reader::new(&body);
    r.skip(14)?;
    let mut blocks = Vec::new();
    let mut types = Vec::new();
    let mut type_footer = 0;
    for (section, size) in [(1, 4), (2, 10), (3, 28), (4, 28)] {
        let n = r.count(limits.max_records)?;
        if section == 4 && n > 256 {
            return Err(Error("RSe type table exceeds 256".into()));
        }
        let payload = r.take(n * size)?;
        if section == 1 {
            blocks = payload
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
        }
        if section == 4 {
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
        if previous < 4 {
            return Err(Error("invalid meta backward span".into()));
        }
        let size = match number {
            8 => Some(20),
            9 => Some(19),
            10 => Some(8),
            11 => Some(4),
            _ => None,
        };
        if let Some(size) = size {
            if count.checked_mul(size) != Some(payload_len) {
                return Err(Error("meta backward section length mismatch".into()));
            }
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
    })
}

fn extended_trailer(r: &mut Reader<'_>) -> Result<()> {
    if r.u8()? == 0 {
        return Ok(());
    }
    let count = r.u32()?;
    if count & 0x8000_0000 != 0 {
        return Ok(());
    }
    if count > 65536 {
        return Err(Error("trailer property limit".into()));
    }
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
    if count > 0 {
        r.skip(8)?;
        for _ in 0..count {
            r.text()?;
            r.skip(4)?;
        }
    }
    Ok(())
}

pub(crate) struct Record {
    pub ordinal: usize,
    pub kind: [u8; 16],
    pub start: usize,
    pub end: usize,
}
pub(crate) fn records(data: &[u8], meta: &Meta, major: u8) -> Result<Vec<Record>> {
    let mut r = Reader::new(data);
    let mut records = Vec::new();
    for (ordinal, block) in meta.blocks.iter().enumerate() {
        if block & 0x8000_0000 == 0 {
            continue;
        }
        let selector = r.u32()? as u8 as usize;
        let kind = *meta
            .types
            .get(selector)
            .ok_or_else(|| Error("absent RSe type index".into()))?;
        let len = (block & 0x7fff_ffff) as usize;
        let start = r.pos;
        r.skip(len)?;
        let end = r.pos;
        let trailing = r.u32()? as usize;
        if trailing != 0 && trailing != len {
            return Err(Error("RSe trailing record length mismatch".into()));
        }
        if major > 18 {
            extended_trailer(&mut r)?;
        }
        records.push(Record {
            ordinal,
            kind,
            start,
            end,
        });
    }
    if r.u32()? != u32::MAX {
        return Err(Error("missing RSe bulk terminal marker".into()));
    }
    // The remainder is a documented opaque stream trailer, never another record.
    Ok(records)
}
