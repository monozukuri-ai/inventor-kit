//! Developer inventory only; no native writer qualification is implied.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path)?;
        let inventory = inventor_core::part::inspect(&bytes, &path, &Default::default())?;
        println!("{}", serde_json::to_string(&inventory)?);
    }
    Ok(())
}
