fn main() {
    let path = std::env::args().nth(1).expect("IPT/IAM path");
    let bytes = std::fs::read(&path).unwrap();
    let doc = inventor_core::assembly::inspect(&bytes, &path, &Default::default()).unwrap();
    println!("{}", serde_json::to_string_pretty(&doc).unwrap());
}
