use crate::{Error, Result};
pub(crate) struct Reader<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| Error("length overflow".into()))?;
        let result = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| Error(format!("truncated structure at byte {}", self.pos)))?;
        self.pos = end;
        Ok(result)
    }
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n)?;
        Ok(())
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn count(&mut self, max: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(Error(format!(
                "count {n} exceeds {max} at {}",
                self.pos - 4
            )));
        }
        Ok(n)
    }
    pub fn utf16(&mut self) -> Result<String> {
        let n = self.count(4096)?;
        let bytes = self.take(n * 2)?;
        String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| Error("invalid UTF-16".into()))
    }
    pub fn text(&mut self) -> Result<String> {
        let n = self.count(65536)?;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| Error("invalid UTF-8".into()))
    }
    pub fn id(&mut self) -> Result<[u8; 16]> {
        Ok(self.take(16)?.try_into().unwrap())
    }
    pub fn finish(&self) -> Result<()> {
        if self.pos != self.bytes.len() {
            Err(Error(format!(
                "unconsumed structure suffix at {} of {}",
                self.pos,
                self.bytes.len()
            )))
        } else {
            Ok(())
        }
    }
}
