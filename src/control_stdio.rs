// MYOS-004: MyKernel DOM UI automation.
//
// Drives the machine from JSON Lines commands read on stdin instead of
// running it to completion, so a host-side test client (qa/mydomtester)
// can launch the kernel headless, query its DOM/accessibility tree, and inject
// mouse input. See issues/tickets/MYOS-004_mykernel-ui-automation.md.
//
// Deliberately not a general JSON protocol: commands are a small fixed set of
// flat objects (`{"cmd":"...", ...}`), so parsing below is a handful of
// substring scans rather than a real JSON parser/serde dependency.
//
// Responses are single JSON-Lines objects printed to stdout, interleaved with
// the guest's own serial output (which the emulator already prints live --
// see memory_bus.rs). A client tells the two apart by prefix: kernel serial
// text is whatever the guest prints, while every control_stdio response
// parses as its own `{"ok":...}` JSON object. dom.snapshot additionally hands
// back the guest's `---DOM-SNAPSHOT-BEGIN/END---`-bracketed lines (already
// individually valid JSON -- see dom.mln's dump_json) re-wrapped as one
// `{"ok":true,"nodes":[...]}` line instead of re-encoding them.
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

use crate::constants::MOUSE_BUTTON_LEFT;
use crate::machine::Machine;

const SNAPSHOT_BEGIN: &str = "---DOM-SNAPSHOT-BEGIN---";
const SNAPSHOT_END: &str = "---DOM-SNAPSHOT-END---";
const BOOT_TIMEOUT: Duration = Duration::from_secs(10);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(2);
// Instruction budgets per machine.run_frame_budget() call -- each call is
// also wall-clock capped (see run_loop.rs), so these are upper bounds, not
// guarantees the guest gets this much CPU per command.
const BOOT_BUDGET: u64 = 500_000;
const INPUT_BUDGET: u64 = 20_000;
const FRAME_WAIT_BUDGET: u64 = 200_000;
const SNAPSHOT_BUDGET: u64 = 50_000;

pub fn run(machine: &mut Machine) -> Result<(), String> {
    wait_for_boot(machine)?;
    ack_ok(None);

    let stdin = io::stdin();
    let mut mouse_x: u32 = 0;
    let mut mouse_y: u32 = 0;
    let mut mouse_buttons: u32 = 0;

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Some(cmd) = json_field_str(line, "cmd") else {
            ack_err("missing \"cmd\"");
            continue;
        };

        match cmd.as_str() {
            "mouse.move" => {
                mouse_x = json_field_i64(line, "x").unwrap_or(mouse_x as i64).max(0) as u32;
                mouse_y = json_field_i64(line, "y").unwrap_or(mouse_y as i64).max(0) as u32;
                machine.set_mouse_state(mouse_x, mouse_y, mouse_buttons);
                machine.run_frame_budget(INPUT_BUDGET)?;
                ack_ok(None);
            }
            // Only the left button is wired up (MOUSE_BUTTON_LEFT is the only
            // one the hardware/kernel define); "button" is accepted but
            // ignored rather than rejected, so the client's API can still
            // name it explicitly.
            "mouse.down" => {
                mouse_buttons |= MOUSE_BUTTON_LEFT;
                machine.set_mouse_state(mouse_x, mouse_y, mouse_buttons);
                machine.run_frame_budget(INPUT_BUDGET)?;
                ack_ok(None);
            }
            "mouse.up" => {
                mouse_buttons &= !MOUSE_BUTTON_LEFT;
                machine.set_mouse_state(mouse_x, mouse_y, mouse_buttons);
                machine.run_frame_budget(INPUT_BUDGET)?;
                ack_ok(None);
            }
            "frame.wait" => {
                machine.run_frame_budget(FRAME_WAIT_BUDGET)?;
                ack_ok(None);
            }
            "dom.snapshot" => dom_snapshot(machine),
            "screenshot" => match json_field_str(line, "path") {
                Some(path) => match machine.write_ppm_screenshot(&path) {
                    Ok(()) => ack_ok(Some(&format!("\"path\":{}", json_quote(&path)))),
                    Err(e) => ack_err(&e),
                },
                None => ack_err("screenshot requires \"path\""),
            },
            other => ack_err(&format!("unknown cmd: {}", other)),
        }
    }

    Ok(())
}

// Pump the machine until the shell prompt shows up in serial output (the
// kernel is ready for commands) or BOOT_TIMEOUT elapses.
fn wait_for_boot(machine: &mut Machine) -> Result<(), String> {
    let deadline = Instant::now() + BOOT_TIMEOUT;
    let mut seen = Vec::new();
    loop {
        machine.run_frame_budget(BOOT_BUDGET)?;
        seen.extend(machine.drain_serial_tx());
        if contains(&seen, b"MyOS>") {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("control-stdio: timed out waiting for the kernel to boot".to_string());
        }
    }
}

// Type "dom\r" at the shell (see shell.mln's `dom` command) and wait for the
// ---DOM-SNAPSHOT-BEGIN/END--- markers dom.dump_json() wraps its output in.
fn dom_snapshot(machine: &mut Machine) {
    machine.ingest_serial_bytes(b"dom\r");

    let deadline = Instant::now() + SNAPSHOT_TIMEOUT;
    let mut captured = Vec::new();
    loop {
        if let Err(e) = machine.run_frame_budget(SNAPSHOT_BUDGET) {
            ack_err(&e);
            return;
        }
        captured.extend(machine.drain_serial_tx());
        if contains(&captured, SNAPSHOT_END.as_bytes()) {
            break;
        }
        if Instant::now() >= deadline {
            ack_err("dom.snapshot timed out waiting for the kernel");
            return;
        }
    }

    match extract_snapshot(&captured) {
        Some(nodes) => {
            println!("\n{{\"ok\":true,\"nodes\":[{}]}}", nodes);
            let _ = io::stdout().flush();
        }
        None => ack_err("dom.snapshot: markers seen but body was not valid UTF-8"),
    }
}

fn extract_snapshot(captured: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(captured);
    let begin = text.find(SNAPSHOT_BEGIN)?;
    let end = text.find(SNAPSHOT_END)?;
    if end <= begin {
        return None;
    }
    let body = &text[begin + SNAPSHOT_BEGIN.len()..end];
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    Some(lines.join(","))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// A leading newline guarantees the JSON starts its own line even when the
// guest's last serial write left the cursor mid-line (e.g. the shell prompt
// "MyOS> " carries no trailing newline -- see shell.mln). Without it a
// client reading stdout line-by-line would see "MyOS> {\"ok\":true}", which
// does not parse as JSON.
fn ack_ok(extra: Option<&str>) {
    match extra {
        Some(e) => println!("\n{{\"ok\":true,{}}}", e),
        None => println!("\n{{\"ok\":true}}"),
    }
    let _ = io::stdout().flush();
}

fn ack_err(msg: &str) {
    println!("\n{{\"ok\":false,\"error\":{}}}", json_quote(msg));
    let _ = io::stdout().flush();
}

fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

// Minimal field extraction for our fixed, flat command shapes -- not a
// general JSON parser. Values containing an escaped quote are not supported.
fn json_field_str(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let key_pos = line.find(&needle)?;
    let after_key = &line[key_pos + needle.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let rest = after_colon.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn json_field_i64(line: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{}\"", key);
    let key_pos = line.find(&needle)?;
    let after_key = &line[key_pos + needle.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let end = after_colon
        .find(|c: char| !(c.is_ascii_digit() || c == '-'))
        .unwrap_or(after_colon.len());
    after_colon[..end].parse::<i64>().ok()
}
