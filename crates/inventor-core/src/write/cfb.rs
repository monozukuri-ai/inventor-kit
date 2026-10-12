use super::*;
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    time::{Duration, SystemTime},
};

pub(super) fn encode(streams: BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    encode_document(streams, "4d29b490-49b2-11d0-93c3-7e0706000000")
}

// IAM and IPT share CFB3 framing, but not their document or segment graphs.
// Selecting a container class does not qualify its streams.
pub(super) fn encode_document(
    streams: BTreeMap<String, Vec<u8>>,
    document_clsid: &str,
) -> Result<Vec<u8>> {
    let mut c =
        ::cfb::CompoundFile::create_with_version(::cfb::Version::V3, Cursor::new(Vec::new()))
            .map_err(err)?;
    let clsid = uuid::Uuid::parse_str(document_clsid).map_err(err)?;
    c.set_storage_clsid("/", clsid).map_err(err)?;
    for path in ["/RSeStorage", "/RSeStorage/V1", "/RSeStorage/RSeEmbeddings"] {
        c.create_storage(path).map_err(err)?;
    }
    if document_clsid == "e60f81e1-49b3-11d0-93c3-7e0706000000" {
        for path in [
            "/RSeStorage/V1/RSeEmbeddings",
            "/RSeStorage/RefdFiles",
            "/RSeStorage/Templates",
        ] {
            c.create_storage(path).map_err(err)?;
        }
    }
    c.set_storage_clsid("/RSeStorage/V1", clsid).map_err(err)?;
    for (path, data) in streams {
        c.create_new_stream(path)
            .map_err(err)?
            .write_all(&data)
            .map_err(err)?;
    }
    let zero = SystemTime::UNIX_EPOCH
        .checked_sub(Duration::from_secs(11_644_473_600))
        .ok_or_else(|| Error("FILETIME origin unavailable".into()))?;
    let paths: Vec<_> = c.walk().map(|e| e.path().to_owned()).collect();
    for path in paths {
        c.set_state_bits(&path, 0).map_err(err)?;
        c.set_created_time(&path, zero).map_err(err)?;
        c.set_modified_time(&path, zero).map_err(err)?;
    }
    c.flush().map_err(err)?;
    Ok(c.into_inner().into_inner())
}
