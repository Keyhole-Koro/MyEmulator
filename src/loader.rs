use std::fs::File;
use std::io::Read;

pub fn read_binary_file(filename: &str) -> Result<Vec<u32>, String> {
    let mut file =
        File::open(filename).map_err(|e| format!("Unable to open binary file: {} ({})", filename, e))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("Failed to read binary file: {} ({})", filename, e))?;

    if bytes.len() % 4 != 0 {
        return Err(format!(
            "Binary file size must be multiple of 4 bytes: {}",
            filename
        ));
    }

    let mut words = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let word = ((chunk[0] as u32) << 24)
            | ((chunk[1] as u32) << 16)
            | ((chunk[2] as u32) << 8)
            | (chunk[3] as u32);
        words.push(word);
    }

    Ok(words)
}
