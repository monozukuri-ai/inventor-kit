//! Drawing-specific ceilings supplement the container/record limits.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DrawingLimits {
    pub max_sheets: usize,
    pub max_views: usize,
    pub max_display_items: usize,
    pub max_polyline_points: usize,
    pub max_text_bytes: usize,
    pub max_reference_visits: usize,
    pub max_nesting_depth: usize,
    pub max_image_bytes: usize,
    pub max_image_pixels: usize,
    pub max_output_bytes: usize,
}
impl Default for DrawingLimits {
    fn default() -> Self {
        Self {
            max_sheets: 256,
            max_views: 4096,
            max_display_items: 100_000,
            max_polyline_points: 1_000_000,
            max_text_bytes: 16 * 1024 * 1024,
            max_reference_visits: 500_000,
            max_nesting_depth: 128,
            max_image_bytes: 16 * 1024 * 1024,
            max_image_pixels: 16_777_216,
            max_output_bytes: 64 * 1024 * 1024,
        }
    }
}
impl DrawingLimits {
    pub fn validate(&self) -> Result<()> {
        let cap = Self::default();
        macro_rules! check {
            ($($field:ident),+) => {$(
                if self.$field > cap.$field {
                    return Err(Error(format!("drawing {} exceeds hard limit {}", stringify!($field), cap.$field)));
                }
            )+};
        }
        check!(
            max_sheets,
            max_views,
            max_display_items,
            max_polyline_points,
            max_text_bytes,
            max_reference_visits,
            max_nesting_depth,
            max_image_bytes,
            max_image_pixels,
            max_output_bytes
        );
        Ok(())
    }

    /// A writer that checks the encoded byte count before extending its buffer.
    pub fn output_buffer(&self) -> Result<DrawingOutputBuffer> {
        self.validate()?;
        Ok(DrawingOutputBuffer {
            bytes: vec![],
            limit: self.max_output_bytes,
        })
    }
}

pub struct DrawingOutputBuffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl DrawingOutputBuffer {
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
    pub fn into_string(self) -> Result<String> {
        String::from_utf8(self.bytes).map_err(|e| Error(e.to_string()))
    }
}
impl Write for DrawingOutputBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() > self.limit - self.bytes.len() {
            return Err(io::Error::other("drawing output byte limit exceeded"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) struct DisplayBudget<'a> {
    pub limits: &'a DrawingLimits,
    pub exhausted: bool,
    items: usize,
    views: usize,
    points: usize,
    text: usize,
}
impl<'a> DisplayBudget<'a> {
    pub fn new(limits: &'a DrawingLimits) -> Self {
        Self {
            limits,
            exhausted: false,
            items: limits.max_display_items,
            views: limits.max_views,
            points: limits.max_polyline_points,
            text: limits.max_text_bytes,
        }
    }
    fn take(left: &mut usize, amount: usize, exhausted: &mut bool, name: &str) -> Result<()> {
        if amount > *left {
            *exhausted = true;
            return Err(Error(format!("drawing {name} limit exceeded")));
        }
        *left -= amount;
        Ok(())
    }
    pub fn item(&mut self) -> Result<()> {
        Self::take(&mut self.items, 1, &mut self.exhausted, "display item")
    }
    pub fn view(&mut self) -> Result<()> {
        Self::take(&mut self.views, 1, &mut self.exhausted, "view")
    }
    pub fn points(&mut self, n: usize) -> Result<()> {
        Self::take(&mut self.points, n, &mut self.exhausted, "polyline point")
    }
    pub fn text(&mut self, n: usize) -> Result<()> {
        Self::take(&mut self.text, n, &mut self.exhausted, "text byte")
    }
}
