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

use crate::constants::{
    KBD_EVT_CHAR, KBD_EVT_DOWN, KBD_EVT_UP, KEY_BACKSPACE, KEY_DELETE, KEY_DOWN, KEY_END,
    KEY_ENTER, KEY_ESCAPE, KEY_HOME, KEY_LEFT, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_RIGHT, KEY_SPACE,
    KEY_TAB, KEY_UP, MOUSE_BUTTON_LEFT, MOUSE_BUTTON_RIGHT,
};
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
const FRAME_WAIT_ROUNDS: u32 = 25;
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
            // "button" defaults to left; "right" selects MOUSE_BUTTON_RIGHT.
            "mouse.down" => {
                mouse_buttons |= mouse_button_from_json(line);
                machine.set_mouse_state(mouse_x, mouse_y, mouse_buttons);
                machine.run_frame_budget(INPUT_BUDGET)?;
                ack_ok(None);
            }
            "mouse.up" => {
                mouse_buttons &= !mouse_button_from_json(line);
                machine.set_mouse_state(mouse_x, mouse_y, mouse_buttons);
                machine.run_frame_budget(INPUT_BUDGET)?;
                ack_ok(None);
            }
            // Keyboard injection. key.type feeds each character of "text" as a
            // CHAR event (what a text field consumes); key.press/key.release
            // queue DOWN/UP for a KEY_* code or a single-character key name.
            "key.type" => match json_field_str(line, "text") {
                Some(text) => {
                    for ch in text.chars() {
                        machine.push_key_event(KBD_EVT_CHAR, ch as u32, 0);
                    }
                    machine.run_frame_budget(INPUT_BUDGET)?;
                    ack_ok(None);
                }
                None => ack_err("key.type requires \"text\""),
            },
            "key.press" | "key.release" => match key_code_from_json(line) {
                Some(code) => {
                    let kind = if cmd == "key.press" { KBD_EVT_DOWN } else { KBD_EVT_UP };
                    let mods = json_field_i64(line, "mods").unwrap_or(0) as u32;
                    machine.push_key_event(kind, code, mods);
                    machine.run_frame_budget(INPUT_BUDGET)?;
                    ack_ok(None);
                }
                None => ack_err("key.press/key.release require \"key\" (a name like \"enter\" or a character)"),
            },
            "mouse.wheel" => {
                let steps = json_field_i64(line, "steps").unwrap_or(0) as i32;
                machine.set_mouse_state_wheel(mouse_x, mouse_y, mouse_buttons, steps);
                machine.run_frame_budget(INPUT_BUDGET)?;
                ack_ok(None);
            }
            // Run until the guest presents a frame (DISPLAY_SWAP), so a
            // following screenshot or snapshot sees the reaction to the
            // inputs above; gives up after FRAME_WAIT_ROUNDS budgets when
            // nothing was dirty and no frame is coming.
            "frame.wait" => {
                let before = machine.swap_count();
                let mut rounds = 0;
                while machine.swap_count() == before && rounds < FRAME_WAIT_ROUNDS {
                    machine.run_frame_budget(FRAME_WAIT_BUDGET)?;
                    rounds += 1;
                }
                ack_ok(None);
            }
            "dom.snapshot" => dom_snapshot(machine),
            "screenshot" => match json_field_str(line, "path") {
                Some(path) => match machine.write_screenshot(&path) {
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

fn mouse_button_from_json(line: &str) -> u32 {
    match json_field_str(line, "button").as_deref() {
        Some("right") => MOUSE_BUTTON_RIGHT,
        _ => MOUSE_BUTTON_LEFT,
    }
}

// "key" is either a named key or a single character (its lowercase ASCII is
// the DOWN/UP code, mirroring keyboard.rs's key_code).
fn key_code_from_json(line: &str) -> Option<u32> {
    let key = json_field_str(line, "key")?;
    let code = match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => KEY_ENTER,
        "backspace" => KEY_BACKSPACE,
        "tab" => KEY_TAB,
        "escape" | "esc" => KEY_ESCAPE,
        "delete" | "del" => KEY_DELETE,
        "space" => KEY_SPACE,
        "left" => KEY_LEFT,
        "right" => KEY_RIGHT,
        "up" => KEY_UP,
        "down" => KEY_DOWN,
        "home" => KEY_HOME,
        "end" => KEY_END,
        "pageup" => KEY_PAGE_UP,
        "pagedown" => KEY_PAGE_DOWN,
        other => {
            let mut chars = other.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            c as u32
        }
    };
    Some(code)
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
