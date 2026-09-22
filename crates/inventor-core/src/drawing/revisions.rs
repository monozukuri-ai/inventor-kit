//! Bounded v3 revision identities and observed major23/31 object-ID intervals.
//! Revision membership establishes stored identity, never current drawing state.
use super::*;
use crate::{property::guid, read::Reader, rse, Error, Result};
use std::collections::BTreeSet;

#[derive(Debug, Serialize)]
pub struct RevisionTable {
    pub source: SourceSpan,
    pub entries: Vec<Revision>,
}
#[derive(Debug, Serialize)]
pub struct Revision {
    pub id: String,
    pub flags: u32,
    pub kind: u16,
    pub source: SourceSpan,
}

pub(super) fn decode(bytes: &[u8], source: SourceSpan, work: &mut usize) -> Result<RevisionTable> {
    let mut r = Reader::new(bytes);
    if r.u32()? != 3 {
        return Err(Error("unsupported drawing revision table version".into()));
    }
    let count = r.count(1_000_000)?;
    rse::charge(work, count)?;
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    for _ in 0..count {
        let start = r.pos;
        let id = guid(&r.id()?);
        if !ids.insert(id.clone()) {
            return Err(Error("duplicate drawing revision identity".into()));
        }
        let flags = r.u32()?;
        let kind = r.u16()?;
        if kind == u16::MAX {
            match r.u8()? {
                0 => r.skip(16)?,
                1 => r.skip(8)?,
                _ => {
                    return Err(Error(
                        "unsupported drawing revision payload selector".into(),
                    ))
                }
            }
        }
        let mut span = source.clone();
        span.start_offset += start;
        span.end_offset = source.start_offset + r.pos;
        entries.push(Revision {
            id,
            flags,
            kind,
            source: span,
        });
    }
    r.finish()?;
    Ok(RevisionTable { source, entries })
}

/// Section 2 holds nondecreasing revision indices and 48-bit upper
/// bounds. The first bound >= an object key identifies its namespace revision;
/// the last entry identifies the stored segment revision. Native resaves and
/// all 484 references in the ten regression drawings exercise this relation.
pub(super) fn binding(
    doc: &DrawingInventory,
    target: &SegmentInventory,
    object: &[u8],
    namespace: &[u8],
    segment: &[u8],
    work: &mut usize,
) -> Result<Vec<SourceSpan>> {
    if object.len() != 8 || namespace.len() != 20 || segment.len() != 32 {
        return Err(Error("invalid revision reference widths".into()));
    }
    let revisions = doc
        .revisions
        .as_ref()
        .ok_or_else(|| Error("missing drawing revision table".into()))?;
    let meta = target
        .meta
        .as_ref()
        .ok_or_else(|| Error("missing target revision ranges".into()))?;
    let mut tables = meta.reference_tables.iter().filter(|t| t.section == 2);
    let table = tables
        .next()
        .ok_or_else(|| Error("missing object revision ranges".into()))?;
    if tables.next().is_some()
        || table.count == 0
        || table.count.checked_mul(10) != Some(table.bytes.len())
        || table
            .source
            .end_offset
            .checked_sub(table.source.start_offset)
            != Some(table.bytes.len())
    {
        return Err(Error("invalid object revision ranges".into()));
    }
    rse::charge(work, table.count)?;
    let key = ((scene::long(&object[2..6]) as u64) << 16) | scene::short(&object[6..]) as u64;
    let mut previous = None;
    let mut identity = None;
    let mut last = None;
    let mut requested_context = None;
    let requested_id = guid(&segment[16..32]);
    for (i, entry) in table.bytes.chunks_exact(10).enumerate() {
        let revision = scene::long(entry) as usize;
        let mut raw = [0u8; 8];
        raw[..6].copy_from_slice(&entry[4..]);
        let upper = u64::from_le_bytes(raw);
        if revision >= revisions.entries.len()
            || previous.is_some_and(|(r, bound)| revision < r || upper < bound)
        {
            return Err(Error("unordered or dangling object revision range".into()));
        }
        if identity.is_none() && key <= upper {
            identity = Some((revision, i));
        }
        if revisions.entries[revision].id == requested_id {
            requested_context = Some((revision, i));
        }
        previous = Some((revision, upper));
        last = Some((revision, i));
    }
    let (identity, slot) =
        identity.ok_or_else(|| Error("object key outside revision ranges".into()))?;
    let (latest, latest_slot) = last.unwrap();
    // Major23 saves may retain a historical target context. Admit it only if
    // the target's own ordered ranges contain it after this object's creation.
    // This binds the stored object key; it does not reconstruct that old state.
    let (current, last_slot) = if target.registry.major == 23 {
        requested_context
            .filter(|(r, _)| *r >= identity)
            .ok_or_else(|| Error("missing or pre-creation target context".into()))?
    } else {
        (latest, latest_slot)
    };
    if revisions.entries[identity].id != guid(&namespace[..16])
        || revisions.entries[current].id != guid(&segment[16..32])
    {
        return Err(Error("display revision/object identity mismatch".into()));
    }
    let span = |slot| {
        let mut s = table.source.clone();
        s.start_offset += slot * 10;
        s.end_offset = s.start_offset + 10;
        s
    };
    let mut evidence = vec![
        span(slot),
        revisions.entries[identity].source.clone(),
        span(last_slot),
        revisions.entries[current].source.clone(),
    ];
    if last_slot != latest_slot {
        evidence.extend([span(latest_slot), revisions.entries[latest].source.clone()]);
    }
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_wire_is_bounded_unique_and_exact() {
        let mut bytes = [3u32.to_le_bytes(), 3u32.to_le_bytes()].concat();
        for (id, kind, selector) in [
            (1u8, 0u16, None),
            (2, 65535, Some(0u8)),
            (3, 65535, Some(1)),
        ] {
            bytes.extend([id; 16]);
            bytes.extend(7u32.to_le_bytes());
            bytes.extend(kind.to_le_bytes());
            if let Some(selector) = selector {
                bytes.push(selector);
                bytes.extend(vec![0; if selector == 0 { 16 } else { 8 }]);
            }
        }
        let parse = |b: &[u8], work: &mut usize| {
            decode(b, SourceSpan::stream("test", "/R", 10, 10 + b.len()), work)
        };
        let table = parse(&bytes, &mut 10).unwrap();
        assert_eq!(table.entries.len(), 3);
        assert_eq!(table.entries[0].source.start_offset, 18);
        assert_eq!(table.entries[2].source.end_offset, 10 + bytes.len());
        assert_eq!(table.entries[1].flags, 7);
        for n in 0..bytes.len() {
            assert!(parse(&bytes[..n], &mut 10).is_err());
        }
        assert!(parse(&bytes, &mut 2).is_err());
        for mutation in 0..4 {
            let mut b = bytes.clone();
            match mutation {
                0 => b.push(0),
                1 => b[0] = 4,
                2 => b[30..46].fill(1), // Duplicate full revision GUID.
                3 => b[52] = 2,         // Unknown variable-payload selector.
                _ => unreachable!(),
            }
            assert!(parse(&b, &mut 10).is_err(), "mutation {mutation}");
        }
    }
}
