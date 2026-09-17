//! Experimental drawing scene, with inventory retained as byte-level evidence.
use std::{error::Error, fs::File, io::Read};
fn main() -> Result<(), Box<dyn Error>> {
    let limits = inventor_core::Limits::default();
    let paths: Vec<_> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        return Err("usage: inspect_drawing_scene FILE.idw [FILE.idw ...]".into());
    }
    for path in paths {
        let mut bytes = Vec::new();
        File::open(&path)?
            .take(limits.max_file_bytes as u64 + 1)
            .read_to_end(&mut bytes)?;
        let result = inventor_core::drawing::inspect(&bytes, &path, &limits)?;
        let preview = inventor_core::drawing::experimental_scene(&result, &limits);
        let sheets = inventor_core::drawing::stored_sheets(&result, &limits);
        let images = inventor_core::drawing::read_embedded_images(&bytes, &preview, &limits)?;
        println!(
            "{}",
            serde_json::json!({"inventory":result,"sheets":sheets,"preview":preview,"images":images})
        );
    }
    Ok(())
}
