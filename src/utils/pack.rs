use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::File;
use std::io::{BufReader, BufWriter};

pub fn do_pack(path: &str, save_path: &str) -> Result<(), String> {
    let input_file = File::open(path).map_err(|e| format!("failed to open {}: {}", path, e))?;
    let output_file_path = format!("{}.z", save_path);
    let output_file = File::create(&output_file_path)
        .map_err(|e| format!("failed to create {}: {}", output_file_path, e))?;

    let mut reader = BufReader::new(input_file);
    let mut encoder = GzEncoder::new(output_file, Compression::best());

    std::io::copy(&mut reader, &mut encoder).map_err(|e| format!("compression failed: {}", e))?;

    encoder
        .finish()
        .map_err(|e| format!("failed to finish compression: {}", e))?;

    Ok(())
}

pub fn un_pack(path: &str, extra: &str) -> Result<(), String> {
    let input_file_path = format!("{}.z", path);
    let input_file = File::open(&input_file_path)
        .map_err(|e| format!("failed to open {}: {}", input_file_path, e))?;

    let output_file_path = format!("{}.{}", path, extra);
    let output_file = File::create(&output_file_path)
        .map_err(|e| format!("failed to create {}: {}", output_file_path, e))?;

    let mut decoder = GzDecoder::new(input_file);
    let mut writer = BufWriter::new(output_file);

    std::io::copy(&mut decoder, &mut writer).map_err(|e| format!("decompression failed: {}", e))?;

    Ok(())
}