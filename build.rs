use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() {
    let config_path =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("config/hardware.conf");
    println!("cargo:rerun-if-changed={}", config_path.display());
    let source = fs::read_to_string(&config_path).expect("read hardware.conf");
    let mut values = BTreeMap::new();
    for line in source.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .expect("hardware config requires key=value");
        let key = key.trim();
        assert!(
            matches!(key, "display_width" | "display_height"),
            "unknown hardware setting: {key}"
        );
        let value: u32 = value
            .trim()
            .parse()
            .expect("dimension must be a positive integer");
        assert!(
            value > 0 && values.insert(key, value).is_none(),
            "invalid or duplicate setting: {key}"
        );
    }
    let width = *values.get("display_width").expect("missing display_width");
    let height = *values
        .get("display_height")
        .expect("missing display_height");
    let bytes = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .expect("framebuffer size overflow");
    assert!(
        bytes <= 0x0100_0000,
        "framebuffer exceeds the 16 MiB VRAM aperture"
    );
    let generated = format!(
        "// Generated from config/hardware.conf.\n         pub const DISPLAY_WIDTH: usize = {width};\n         pub const DISPLAY_HEIGHT: usize = {height};\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("display_constants.rs"),
        generated,
    )
    .expect("write display constants");
}
