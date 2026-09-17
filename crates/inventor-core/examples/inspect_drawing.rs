//! Structural report only; never interprets segment names as active sheets.
use std::{error::Error, fs::File, io::Read};
fn main() -> Result<(), Box<dyn Error>> {
    let limits = inventor_core::Limits::default();
    let paths: Vec<_> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        return Err("usage: inspect_drawing FILE.idw [FILE.idw ...]".into());
    }
    for path in paths {
        let mut bytes = Vec::new();
        File::open(&path)?
            .take(limits.max_file_bytes as u64 + 1)
            .read_to_end(&mut bytes)?;
        let result = inventor_core::drawing::inspect(&bytes, &path, &limits)?;
        println!("{}", serde_json::to_string(&result)?);
    }
    Ok(())
}
