#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! stl_io = "0.8"
//! ```

use std::fs::File;
use std::io::{BufReader, BufWriter};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(".")? {
        let path = entry?.path();

        if path.extension().and_then(|e| e.to_str()) != Some("stl") {
            continue;
        }

        let mut input = BufReader::new(File::open(&path)?);
        let mesh = stl_io::read_stl(&mut input)?;

        let mut output = BufWriter::new(File::create(path)?);
        stl_io::write_stl(&mut output, mesh.into_triangle_vec().into_iter())?;
    }

    Ok(())
}
