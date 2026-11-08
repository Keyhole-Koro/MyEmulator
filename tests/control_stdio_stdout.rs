// In --control-stdio mode stdout carries only the JSON Lines protocol. The
// guest runs while a request is being served, so anything the emulator prints
// for it then -- the DEBUG instruction's registers and memory dump, the
// --verbose instruction log -- has to go to stderr, or the client reads it as
// a broken reply.
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn guest_diagnostics_stay_off_the_protocol_stream() {
    let dir = std::env::temp_dir().join(format!("myemu-control-stdio-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let program: [u32; 4] = [
        0x1A << 26,          // DEBUG: print the registers, dump the top of RAM
        (0x02 << 26) | 1,    // MOVI r0, 1
        (0x17 << 26) | 0x38, // OUT r0 to DISPLAY_SWAP, which ends the key press below
        0x3F << 26,          // HALT
    ];
    let image = dir.join("debug.bin");
    let bytes: Vec<u8> = program.iter().flat_map(|word| word.to_be_bytes()).collect();
    std::fs::write(&image, bytes).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_myemu"))
        .arg("-i")
        .arg(&image)
        .args(["--control-stdio", "--verbose"])
        .current_dir(&dir) // DEBUG writes its memory dump here
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A key press runs the guest until the display swaps; closing stdin then
    // ends the session.
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, r#"{{"id":1,"method":"input.key.press","params":{{"key":"a"}}}}"#).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "myemu failed:\n{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "stdout holds more than the reply:\n{stdout}");
    let reply: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["ok"], true);

    // The diagnostics still reach whoever runs the emulator.
    assert!(stderr.contains("R0: 0x00000000"), "no register dump on stderr:\n{stderr}");
    assert!(stderr.contains("Memory dump written to"), "no dump message on stderr:\n{stderr}");
    assert!(stderr.contains("Raw: 0x68000000"), "no instruction log on stderr:\n{stderr}");
}
