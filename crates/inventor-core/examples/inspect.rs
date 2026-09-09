fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap();
        match inventor_core::read(&bytes, &path, &Default::default()) {
            Ok(doc) => {
                println!("{}", serde_json::to_string(&doc.summary).unwrap());
                if let Some(model) = doc.model {
                    eprintln!(
                        "{path}: {} records, {} bodies",
                        model.len(),
                        model.bodies().count()
                    );
                    for d in model
                        .diagnostics()
                        .iter()
                        .filter(|d| d.code == "sab.entity_schema_unsupported")
                        .take(10)
                    {
                        eprintln!("{}", d.message);
                    }
                }
                if let Some(bytes) = doc.kernel_bytes {
                    if let Ok(dir) = std::env::var("INVENTOR_PROBE_KERNEL_DIR") {
                        std::fs::create_dir_all(&dir).unwrap();
                        std::fs::write(
                            std::path::Path::new(&dir)
                                .join(std::path::Path::new(&path).file_name().unwrap())
                                .with_extension("sab"),
                            bytes,
                        )
                        .unwrap();
                    }
                }
            }
            Err(e) => eprintln!("{path}: {e}"),
        }
    }
}
