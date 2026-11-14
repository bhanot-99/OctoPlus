//! Generates Rust types from `proto/neuroos/v1` with the system `protoc`
//! (the same compiler the C++ build uses, so both sides read one definition).

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../proto");
    let dir = root.join("neuroos/v1");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "proto"))
        .collect();
    files.sort();
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
    }
    println!("cargo:rerun-if-changed={}", dir.display());
    prost_build::Config::new().compile_protos(&files, &[root])?;
    Ok(())
}
