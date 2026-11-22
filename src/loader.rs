use std::fs::File;
use std::io::Read;

pub fn read_binary_file(filename: &str) -> Result<Vec<u32>, String> {
    let mut file = File::open(filename)
        .map_err(|e| format!("Unable to open binary file: {} ({})", filename, e))?;

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
    for chunk in bytes.as_chunks::<4>().0 {
        let word = ((chunk[0] as u32) << 24)
            | ((chunk[1] as u32) << 16)
            | ((chunk[2] as u32) << 8)
            | (chunk[3] as u32);
        words.push(word);
    }

    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::read_binary_file;
    use std::path::PathBuf;

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "myemu-loader-test-{}-{}.bin",
            name,
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn words_are_big_endian() {
        let path = temp_file("be", &[0x12, 0x34, 0x56, 0x78, 0xFF, 0x00, 0x00, 0x01]);
        let words = read_binary_file(path.to_str().unwrap());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(words.unwrap(), vec![0x1234_5678, 0xFF00_0001]);
    }

    #[test]
    fn an_empty_file_is_no_words() {
        let path = temp_file("empty", &[]);
        let words = read_binary_file(path.to_str().unwrap());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(words.unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn a_size_that_is_not_whole_words_is_an_error() {
        let path = temp_file("odd", &[1, 2, 3, 4, 5]);
        let name = path.to_str().unwrap().to_string();
        let result = read_binary_file(&name);
        std::fs::remove_file(&path).unwrap();
        let error = result.unwrap_err();
        assert!(error.contains("multiple of 4"), "{error}");
        assert!(error.contains(&name), "{error}");
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "myemu-loader-test-missing-{}.bin",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let error = read_binary_file(path.to_str().unwrap()).unwrap_err();
        assert!(error.starts_with("Unable to open binary file"), "{error}");
    }
}
