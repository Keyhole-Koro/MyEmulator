// trace.start, trace.read, trace.write and trace.stop over --control-stdio: a
// trace logs calls to the watched entries, direct or through a function value,
// in the order they ran, and marks where each request let the guest run.
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const MOVI: u32 = 0x02 << 26;
const MOV: u32 = 0x01 << 26;
const CALL: u32 = 0x1B << 26;
const OUT: u32 = 0x17 << 26;
const HALT: u32 = 0x3F << 26;
const PC: u32 = 0x08;
const LR: u32 = 0x0C;

fn session(dir: &Path, profile: bool, requests: &[Value]) -> Vec<Value> {
    let program: [u32; 11] = [
        MOVI | (2 << 21) | 0x20,       // 0x00: a function value
        MOVI | (LR << 21) | 0x0C,      // 0x04: where it returns
        MOV | (PC << 21) | (2 << 16),  // 0x08: call it
        CALL | 0x1C,                   // 0x0C: call 0x28
        MOVI | 1,                      // 0x10
        OUT | 0x38,                    // 0x14: DISPLAY_SWAP ends the key press
        HALT,                          // 0x18
        0,                             // 0x1C
        MOVI | (1 << 21) | 7,          // 0x20
        MOV | (PC << 21) | (LR << 16), // 0x24: return
        MOV | (PC << 21) | (LR << 16), // 0x28: return
    ];
    let image = dir.join("trace.bin");
    let bytes: Vec<u8> = program.iter().flat_map(|word| word.to_be_bytes()).collect();
    std::fs::write(&image, bytes).unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_myemu"));
    command.arg("-i").arg(&image).arg("--control-stdio");
    if profile {
        command.arg("--profile").arg(dir.join("profile.json"));
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for (id, request) in requests.iter().enumerate() {
        let mut request = request.clone();
        request["id"] = json!(id);
        writeln!(stdin, "{request}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "myemu failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn temporary(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("myemu-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// (kind, pc or method, retired) for each event.
fn summary(events: &Value) -> Vec<(String, Value, u64)> {
    events
        .as_array()
        .unwrap()
        .iter()
        .map(|event| {
            let what = event
                .get("pc")
                .or_else(|| event.get("method"))
                .cloned()
                .unwrap_or(Value::Null);
            (
                event["kind"].as_str().unwrap().to_string(),
                what,
                event["retired"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_trace_logs_watched_calls_in_order() {
    let dir = temporary("trace");
    let path = dir.join("trace.json");
    let replies = session(
        &dir,
        true,
        &[
            json!({"method": "trace.read"}),
            json!({"method": "trace.start", "params": {"watch": [0x20, {"pc": 0x28, "space": 0}]}}),
            json!({"method": "input.key.press", "params": {"key": "a"}}),
            json!({"method": "trace.read", "params": {"since": 1}}),
            json!({"method": "trace.write", "params": {"path": path.to_str().unwrap()}}),
            json!({"method": "trace.stop"}),
        ],
    );
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    assert_eq!(replies.len(), 6);
    assert_eq!(replies[0]["error"]["code"], "trace_off", "{}", replies[0]);
    assert_eq!(replies[1]["result"]["watching"], 2, "{}", replies[1]);
    assert_eq!(replies[2]["ok"], true, "{}", replies[2]);

    let read = &replies[3]["result"];
    assert_eq!(read["next"], 5);
    let expected = vec![
        ("enter".to_string(), json!(0x20), 3),
        ("exit".to_string(), json!(0x20), 5),
        ("enter".to_string(), json!(0x28), 6),
        ("exit".to_string(), json!(0x28), 7),
    ];
    assert_eq!(summary(&read["events"]), expected);

    assert_eq!(written["format"], 1);
    let all = summary(&written["events"]);
    assert_eq!(all[0], ("request".to_string(), json!("input.key.press"), 0));
    assert_eq!(all[1..], expected[..]);
    assert_eq!(replies[4]["result"]["events"], 5);
    assert_eq!(
        replies[5]["result"],
        json!({"events": 5, "truncated": false})
    );
}

#[test]
fn a_trace_needs_the_profiler() {
    let dir = temporary("trace-off");
    let replies = session(
        &dir,
        false,
        &[json!({"method": "trace.start", "params": {"watch": [0x20]}})],
    );
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(replies[0]["error"]["code"], "profile_off", "{}", replies[0]);
}
