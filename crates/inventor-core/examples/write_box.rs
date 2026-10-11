//! Development M3 entrypoint; writes only a new file and consumes numeric input.
use acis_core::{authoring::BoxSpec, Vec3};
use inventor_core::write::BoxDocument;
use std::{fs::OpenOptions, io::Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().skip(1).collect();
    if a.len() != 7 && a.len() != 10 {
        return Err("output.ipt size-x size-y size-z origin-x origin-y origin-z [document-name feature-name body-name]".into());
    }
    let n: Vec<f64> = a[1..7]
        .iter()
        .map(|s| s.parse())
        .collect::<Result<_, _>>()?;
    let mut doc = BoxDocument::new(BoxSpec::new(
        Vec3 {
            x: n[0],
            y: n[1],
            z: n[2],
        },
        Vec3 {
            x: n[3],
            y: n[4],
            z: n[5],
        },
    ));
    if a.len() == 10 {
        doc.document_name = a[7].clone();
        doc.feature_name = a[8].clone();
        doc.body_name = a[9].clone();
    }
    let bytes = doc.encode()?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&a[0])?
        .write_all(&bytes)?;
    println!(
        "{} bytes; native_qualified={}",
        bytes.len(),
        inventor_core::write::NATIVE_QUALIFIED
    );
    Ok(())
}
