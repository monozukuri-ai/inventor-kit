//! Experimental whole-IPT authoring from semantic input, without native APIs.
//!
//! The default route emits one metric box and one nonparametric base feature.
//! A separately gated development route supports bounded analytic solids.
//! The profile is frozen to Inventor 2027.1 RSe31. Five generated cases passed the
//! native M3 gate on build 311270010; self-reading alone does not establish acceptance.
#[cfg(feature = "analytic-writer")]
pub mod analytic;
mod carrier;
mod cfb;
mod document;
mod geometry;
mod kernel_labels;
mod profile;
mod properties;
mod rse;

use crate::{Error, Result};
use acis_core::authoring::BoxSpec;
use serde_json::Value;

pub const PROFILE_ID: &str =
    "inventor-ipt-cfb3-rse31-metric-single-base-feature-box-app-root-table-v44";
/// The frozen profile passed M3 for five box cases on Inventor 2027.1 build 311270010.
/// This is profile qualification, not certification of arbitrary input documents.
pub const NATIVE_QUALIFIED: bool = true;
// Initial base-feature anchor shared by the result SAB and its RSe history.
// Qualified for creation; not a rule for subsequent native edit states.
const BASE_FEATURE_STATE: u32 = 3;
const SOURCE_STATE: u32 = 2;

/// No parsed document, native file, payload or template enters this API.
#[derive(Debug, Clone)]
pub struct BoxDocument {
    pub shape: BoxSpec,
    pub document_name: String,
    pub feature_name: String,
    pub body_name: String,
}
impl BoxDocument {
    pub fn new(shape: BoxSpec) -> Self {
        Self {
            shape,
            document_name: "Authored box".into(),
            feature_name: "Base feature".into(),
            body_name: "Solid1".into(),
        }
    }

    /// Assemble an entire CFB3 file in memory. Does not read the filesystem.
    pub fn encode(&self) -> Result<Vec<u8>> {
        for name in [&self.document_name, &self.feature_name, &self.body_name] {
            if name.is_empty() || name.contains('\0') || name.encode_utf16().count() > 255 {
                return Err(Error(
                    "writer name must contain 1..255 UTF-16 units without NUL".into(),
                ));
            }
        }
        let shape = self.shape.build().map_err(err)?;
        let p = profile::Profile::load()?;
        let graph = document::Graph::new(p, self, shape)?;
        let streams = rse::streams(&graph)?;
        let bytes = cfb::encode(streams)?;
        // Independent reader checks the generated container, framing and SABs.
        let read = crate::read(&bytes, "source-free-box", &crate::Limits::default())?;
        if read.summary.kind != "part" || read.model.is_none() {
            return Err(Error(format!(
                "writer self-read: status={} diagnostics={:?} geometry={:?}",
                read.summary.status, read.summary.diagnostics, read.summary.geometry
            )));
        }
        let inventory = crate::part::inspect(&bytes, "source-free-box", &crate::Limits::default())?;
        if inventory.segments.len() != 8
            || inventory
                .segments
                .iter()
                .map(|s| s.records.len())
                .sum::<usize>()
                != 789
        {
            return Err(Error(
                "writer self-read segment/record census differs".into(),
            ));
        }
        Ok(bytes)
    }
}

fn err(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| Error(format!("missing profile text field {key}")))
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value]> {
    v[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| Error(format!("missing profile array {key}")))
}
fn number(v: &Value) -> Result<u32> {
    v.as_u64()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| Error("profile integer outside u32".into()))
}
fn guid(text: &str) -> Result<[u8; 16]> {
    let id = uuid::Uuid::parse_str(text).map_err(err)?;
    if id.is_nil() {
        return Err(Error("nil profile GUID".into()));
    }
    Ok(id.to_bytes_le())
}

#[derive(Default)]
struct Wire(Vec<u8>);
impl Wire {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend(v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend(v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend(v.to_le_bytes());
    }
    fn words(&mut self, v: &[u32]) {
        for &v in v {
            self.u32(v);
        }
    }
    fn id(&mut self, v: &[u8; 16]) {
        self.0.extend(v);
    }
    fn utf16(&mut self, v: &str) {
        self.u32(v.encode_utf16().count() as u32);
        for c in v.encode_utf16() {
            self.u16(c);
        }
    }
    fn text(&mut self, v: &str) {
        self.u32(v.len() as u32);
        self.0.extend(v.as_bytes());
    }
    fn field(&mut self, kind: &str, v: &Value) -> Result<()> {
        match kind {
            "u8" => self.u8(number(v)?.try_into().map_err(err)?),
            "u16" => self.u16(number(v)?.try_into().map_err(err)?),
            "u32" => self.u32(number(v)?),
            "i32" => self.0.extend(
                i32::try_from(
                    v.as_i64()
                        .ok_or_else(|| Error("invalid signed field".into()))?,
                )
                .map_err(err)?
                .to_le_bytes(),
            ),
            "f32" | "f64" => {
                let n = v
                    .as_f64()
                    .filter(|x| x.is_finite())
                    .ok_or_else(|| Error("nonfinite field".into()))?;
                if kind == "f64" {
                    self.f64(n);
                } else {
                    let n = n as f32;
                    if !n.is_finite() {
                        return Err(Error("field overflows f32".into()));
                    }
                    self.0.extend(n.to_le_bytes());
                }
            }
            "utf16" => self.utf16(
                v.as_str()
                    .ok_or_else(|| Error("invalid text field".into()))?,
            ),
            "ascii" => {
                let t = v
                    .as_str()
                    .filter(|x| x.is_ascii())
                    .ok_or_else(|| Error("invalid ASCII field".into()))?;
                self.text(t);
            }
            // Explicit nil defaults are permitted in fixed field recipes. Registry
            // and class identity constructors use the non-nil `guid` validator.
            "guid" => self.id(&uuid::Uuid::parse_str(
                v.as_str()
                    .ok_or_else(|| Error("invalid GUID field".into()))?,
            )
            .map_err(err)?
            .to_bytes_le()),
            _ => return Err(Error(format!("unsupported typed field {kind}"))),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
