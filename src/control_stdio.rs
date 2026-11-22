//! Version 2 of the MyOS automation protocol.
//!
//! stdin/stdout are exclusively JSON Lines. Guest serial is redirected to
//! stderr by `Machine::enable_control_stdio`, and DOM data travels over the
//! automation MMIO bridge instead of by typing commands into the OS shell.
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::constants::{
    KBD_EVT_CHAR, KBD_EVT_DOWN, KBD_EVT_UP, KEY_BACKSPACE, KEY_DELETE, KEY_DOWN, KEY_END,
    KEY_ENTER, KEY_ESCAPE, KEY_HOME, KEY_LEFT, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_RIGHT,
    KEY_SPACE, KEY_TAB, KEY_UP, MOUSE_BUTTON_LEFT, MOUSE_BUTTON_MIDDLE, MOUSE_BUTTON_RIGHT,
};
use crate::machine::{Machine, TraceOptions};

const BRIDGE_DOM_SNAPSHOT: u32 = 1;
const BRIDGE_DOM_HIT_TEST: u32 = 2 << 28;
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_BUDGET: u64 = 100_000;

#[derive(Deserialize)]
struct Request {
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

pub fn run(machine: &mut Machine) -> Result<(), String> {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() { continue; }
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                reply_error(Value::Null, "invalid_request", &error.to_string(), Value::Null);
                continue;
            }
        };
        handle(machine, request);
    }
    Ok(())
}

type MethodError = (&'static str, String, Value);

fn profile_reset(machine: &mut Machine) -> Result<Value, MethodError> {
    if !machine.reset_profile() {
        return Err(("profile_off", "start myemu with --profile".to_string(), Value::Null));
    }
    Ok(json!({"reset": true}))
}

fn profile_write(machine: &Machine, params: &Value) -> Result<Value, MethodError> {
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(("invalid_params", "profile.write needs a path".to_string(), Value::Null));
    };
    machine
        .write_profile(path)
        .map_err(|message| ("profile_failed", message, Value::Null))?;
    Ok(json!({"path": path}))
}

// A trace needs the profiler's call stacks, and a log to read.
fn trace_available(machine: &Machine, started: bool) -> Result<(), MethodError> {
    if !machine.has_profiler() {
        return Err(("profile_off", "start myemu with --profile".to_string(), Value::Null));
    }
    if started && !machine.has_trace() {
        return Err(("trace_off", "no trace was started".to_string(), Value::Null));
    }
    Ok(())
}

// trace.start: log calls to the watched function entries, each a PC or a
// {"pc", "space"} pair for one address space only; "all" logs every call.
// "interrupts" (default true) adds interrupts, their returns and thread
// switches. "limit" caps the events kept.
fn trace_start(machine: &mut Machine, params: &Value) -> Result<Value, MethodError> {
    trace_available(machine, false)?;
    let mut options = TraceOptions::default();
    let watch = match params.get("watch") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(entries)) => entries.as_slice(),
        Some(other) => return Err(("invalid_params", "watch must be an array".to_string(), other.clone())),
    };
    for entry in watch {
        match entry {
            Value::Object(_) => {
                let pc = u32_param(entry, "pc")?;
                match entry.get("space") {
                    None | Some(Value::Null) => { options.pcs.insert(pc); }
                    Some(_) => { options.spaced.insert((u32_param(entry, "space")?, pc)); }
                }
            }
            _ => {
                let pc = entry.as_u64().and_then(|pc| u32::try_from(pc).ok()).ok_or_else(|| {
                    ("invalid_params", "a watched entry is a PC or {\"pc\", \"space\"}".to_string(), entry.clone())
                })?;
                options.pcs.insert(pc);
            }
        }
    }
    options.all = bool_param(params, "all", false)?;
    options.interrupts = bool_param(params, "interrupts", true)?;
    if let Some(limit) = params.get("limit").filter(|value| !value.is_null()) {
        options.limit = limit.as_u64().map(|n| n as usize)
            .ok_or_else(|| ("invalid_params", "limit must be an unsigned integer".to_string(), limit.clone()))?;
    }
    let watching = options.pcs.len() + options.spaced.len();
    machine.start_trace(options);
    Ok(json!({"watching": watching}))
}

fn trace_stop(machine: &mut Machine) -> Result<Value, MethodError> {
    trace_available(machine, true)?;
    let (events, truncated) = machine.stop_trace().unwrap_or_default();
    Ok(json!({"events": events, "truncated": truncated}))
}

fn trace_write(machine: &Machine, params: &Value) -> Result<Value, MethodError> {
    trace_available(machine, true)?;
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return Err(("invalid_params", "trace.write needs a path".to_string(), Value::Null));
    };
    let (events, truncated) = machine
        .write_trace(path)
        .map_err(|message| ("trace_failed", message, Value::Null))?;
    Ok(json!({"path": path, "events": events, "truncated": truncated}))
}

// trace.read: the log itself, from event "since" on (default 0), so a client
// can follow a running trace without reading it all again.
fn trace_read(machine: &Machine, params: &Value) -> Result<Value, MethodError> {
    trace_available(machine, true)?;
    let since = match params.get("since") {
        None | Some(Value::Null) => 0,
        Some(_) => u32_param(params, "since")? as usize,
    };
    Ok(machine.read_trace(since).unwrap_or(Value::Null))
}

fn handle(machine: &mut Machine, request: Request) {
    // The guest runs while a request is served; a trace shows where each began.
    if !request.method.starts_with("trace.") {
        machine.trace_request(&request.method);
    }
    let result = match request.method.as_str() {
        "session.hello" => Ok(json!({
            "protocol": 2,
            "capabilities": ["dom.snapshot", "dom.hit_test", "input.pointer.sequence", "input.key.type", "input.key.press", "screen.screenshot", "profile.reset", "profile.write", "trace.start", "trace.stop", "trace.write", "trace.read"]
        })),
        "os.ready" => wait_for_dom(machine).and_then(|_| quiesce(machine)).map(|_| json!({"ready": true})),
        "dom.snapshot" => dom_snapshot(machine),
        "dom.hit_test" => dom_hit_test(machine, &request.params),
        "input.pointer.sequence" => pointer_sequence(machine, &request.params),
        "input.key.type" => key_type(machine, &request.params),
        "input.key.press" => key_press(machine, &request.params),
        "screen.screenshot" => screenshot(machine, &request.params),
        "screen.cursor" => cursor(machine),
        "timeline.start" => {
            machine.start_timeline();
            Ok(json!({}))
        }
        "timeline.write" => timeline_write(machine, &request.params),
        "profile.reset" => profile_reset(machine),
        "profile.write" => profile_write(machine, &request.params),
        "trace.start" => trace_start(machine, &request.params),
        "trace.stop" => trace_stop(machine),
        "trace.write" => trace_write(machine, &request.params),
        "trace.read" => trace_read(machine, &request.params),
        _ => Err(("unknown_method", format!("unknown method: {}", request.method), Value::Null)),
    };
    match result {
        Ok(value) => reply_ok(request.id, value),
        Err((code, message, details)) => reply_error(request.id, code, &message, details),
    }
}

fn wait_for_dom(machine: &mut Machine) -> Result<Vec<u8>, (&'static str, String, Value)> {
    bridge_request(machine, BRIDGE_DOM_SNAPSHOT)
}

fn bridge_request(machine: &mut Machine, command: u32) -> Result<Vec<u8>, (&'static str, String, Value)> {
    machine.automation_request(command).map_err(|e| ("bridge_busy", e, Value::Null))?;
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        machine.run_frame_budget(COMMAND_BUDGET).map_err(|e| ("machine_error", e, Value::Null))?;
        if machine.automation_done() { return Ok(machine.take_automation_response()); }
    }
    Err(("not_ready", machine.describe_error("timed out waiting for the MyOS automation service".to_string()), Value::Null))
}

fn dom_snapshot(machine: &mut Machine) -> Result<Value, (&'static str, String, Value)> {
    let bytes = wait_for_dom(machine)?;
    serde_json::from_slice(&bytes).map_err(|error| {
        ("invalid_dom_response", format!("automation bridge returned invalid JSON: {}", error), json!({"body": String::from_utf8_lossy(&bytes)}))
    })
}

fn dom_hit_test(machine: &mut Machine, params: &Value) -> Result<Value, (&'static str, String, Value)> {
    let x = u32_param(params, "x")?;
    let y = u32_param(params, "y")?;
    if x >= (1 << 14) || y >= (1 << 14) {
        return Err(("invalid_params", "coordinates exceed automation bridge range".to_string(), params.clone()));
    }
    let raw = bridge_request(machine, BRIDGE_DOM_HIT_TEST | (x << 14) | y)?;
    serde_json::from_slice(&raw).map_err(|error| {
        ("invalid_dom_response", format!("automation bridge returned invalid JSON: {}", error), Value::Null)
    })
}

fn pointer_sequence(machine: &mut Machine, params: &Value) -> Result<Value, (&'static str, String, Value)> {
    let events = params.get("events").and_then(Value::as_array)
        .ok_or_else(|| ("invalid_params", "events must be an array".to_string(), Value::Null))?;
    let mut x = 0u32;
    let mut y = 0u32;
    let mut buttons = 0u32;
    for event in events {
        let kind = event.get("type").and_then(Value::as_str)
            .ok_or_else(|| ("invalid_params", "each pointer event needs a type".to_string(), event.clone()))?;
        match kind {
            "move" => { x = u32_param(event, "x")?; y = u32_param(event, "y")?; }
            "down" => buttons |= button_param(event)?,
            "up" => buttons &= !button_param(event)?,
            _ => return Err(("invalid_params", format!("unknown pointer event: {}", kind), event.clone())),
        }
        machine.set_mouse_state(x, y, buttons);
        machine.run_frame_budget(COMMAND_BUDGET).map_err(|e| ("machine_error", e, Value::Null))?;
    }
    settle(machine)?;
    Ok(json!({"frame": machine.swap_count()}))
}

fn key_type(machine: &mut Machine, params: &Value) -> Result<Value, (&'static str, String, Value)> {
    let text = params.get("text").and_then(Value::as_str)
        .ok_or_else(|| ("invalid_params", "text must be a string".to_string(), Value::Null))?;
    for ch in text.chars() { machine.push_key_event(KBD_EVT_CHAR, ch as u32, 0); }
    settle(machine)?;
    Ok(json!({"frame": machine.swap_count()}))
}

fn key_press(machine: &mut Machine, params: &Value) -> Result<Value, (&'static str, String, Value)> {
    let key = params.get("key").and_then(Value::as_str)
        .ok_or_else(|| ("invalid_params", "key must be a string".to_string(), Value::Null))?;
    let mods = params.get("mods").and_then(Value::as_u64).unwrap_or(0) as u32;
    let code = key_code(key).ok_or_else(|| ("invalid_params", format!("unknown key: {}", key), Value::Null))?;
    machine.push_key_event(KBD_EVT_DOWN, code, mods);
    machine.push_key_event(KBD_EVT_UP, code, mods);
    settle(machine)?;
    Ok(json!({"frame": machine.swap_count()}))
}

// Writes the timeline recorded since timeline.start (or --timeline) and stops.
fn timeline_write(machine: &mut Machine, params: &Value) -> Result<Value, (&'static str, String, Value)> {
    let path = params.get("path").and_then(Value::as_str)
        .ok_or_else(|| ("invalid_params", "path must be a string".to_string(), Value::Null))?;
    machine.write_timeline(path).map_err(|e| ("timeline_failed", e, Value::Null))?;
    Ok(json!({"path": path}))
}

// Where the hardware cursor is, as the guest last set it. Read-only: the
// guest does not run for it.
fn cursor(machine: &mut Machine) -> Result<Value, (&'static str, String, Value)> {
    let (x, y, visible, shape) = machine.cursor_state();
    Ok(json!({"x": x, "y": y, "visible": visible, "shape": shape}))
}

fn screenshot(machine: &mut Machine, params: &Value) -> Result<Value, (&'static str, String, Value)> {
    let path = params.get("path").and_then(Value::as_str)
        .ok_or_else(|| ("invalid_params", "path must be a string".to_string(), Value::Null))?;
    machine.write_screenshot(path).map_err(|e| ("screenshot_failed", e, Value::Null))?;
    Ok(json!({"path": path}))
}

// Boot is not over when the automation service first answers: the shell
// is still starting the desktop's apps, each a process building its window
// over the UI protocol, and the guest only runs while a request is being
// served. Run it on until the shell reports no app still starting (the
// snapshot's "starting" count), so a test's first click lands on a settled
// desktop rather than paying for the rest of the boot inside its own timeout.
fn quiesce(machine: &mut Machine) -> Result<(), (&'static str, String, Value)> {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        let snapshot = dom_snapshot(machine)?;
        let starting = snapshot.get("starting").and_then(Value::as_u64).unwrap_or(0);
        if starting == 0 { return Ok(()); }
        // Let the guest work between looks.
        for _ in 0..20 {
            machine.run_frame_budget(COMMAND_BUDGET).map_err(|e| ("machine_error", e, Value::Null))?;
        }
    }
    Ok(())
}

fn settle(machine: &mut Machine) -> Result<(), (&'static str, String, Value)> {
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let before = machine.swap_count();
    while Instant::now() < deadline {
        machine.run_frame_budget(COMMAND_BUDGET).map_err(|e| ("machine_error", e, Value::Null))?;
        if machine.swap_count() != before { return Ok(()); }
    }
    Ok(()) // Inputs which do not repaint are still valid.
}

fn u32_param(value: &Value, key: &str) -> Result<u32, (&'static str, String, Value)> {
    // try_from rather than `as`: 2^32 must be rejected, not read as 0.
    value.get(key).and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| ("invalid_params", format!("{} must be an unsigned integer", key), value.clone()))
}

fn bool_param(value: &Value, key: &str, default: bool) -> Result<bool, MethodError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(flag)) => Ok(*flag),
        Some(other) => Err(("invalid_params", format!("{} must be true or false", key), other.clone())),
    }
}

fn button_param(value: &Value) -> Result<u32, (&'static str, String, Value)> {
    match value.get("button").and_then(Value::as_str).unwrap_or("left") {
        "left" => Ok(MOUSE_BUTTON_LEFT), "right" => Ok(MOUSE_BUTTON_RIGHT), "middle" => Ok(MOUSE_BUTTON_MIDDLE),
        button => Err(("invalid_params", format!("unknown mouse button: {}", button), value.clone())),
    }
}

fn key_code(key: &str) -> Option<u32> {
    Some(match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => KEY_ENTER, "backspace" => KEY_BACKSPACE, "tab" => KEY_TAB,
        "escape" | "esc" => KEY_ESCAPE, "delete" | "del" => KEY_DELETE, "space" => KEY_SPACE,
        "left" => KEY_LEFT, "right" => KEY_RIGHT, "up" => KEY_UP, "down" => KEY_DOWN,
        "home" => KEY_HOME, "end" => KEY_END, "pageup" => KEY_PAGE_UP, "pagedown" => KEY_PAGE_DOWN,
        single if single.chars().count() == 1 => single.chars().next()? as u32,
        _ => return None,
    })
}

fn reply_ok(id: Value, result: Value) { reply(json!({"id": id, "ok": true, "result": result})); }
fn reply_error(id: Value, code: &str, message: &str, details: Value) {
    reply(json!({"id": id, "ok": false, "error": {"code": code, "message": message, "details": details}}));
}
fn reply(value: Value) {
    let mut stdout = io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, &value);
    let _ = writeln!(stdout);
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::{bool_param, button_param, key_code, u32_param};
    use crate::constants::{
        KEY_ENTER, KEY_ESCAPE, KEY_PAGE_DOWN, MOUSE_BUTTON_LEFT, MOUSE_BUTTON_MIDDLE,
        MOUSE_BUTTON_RIGHT,
    };
    use serde_json::json;

    #[test]
    fn u32_param_accepts_the_whole_u32_range() {
        assert_eq!(u32_param(&json!({"x": 0}), "x").unwrap(), 0);
        assert_eq!(
            u32_param(&json!({"x": 4294967295u64}), "x").unwrap(),
            u32::MAX
        );
    }

    #[test]
    fn u32_param_rejects_what_does_not_fit() {
        for params in [
            json!({"x": 4294967296u64}),
            json!({"x": 4294967297u64}),
            json!({"x": u64::MAX}),
            json!({"x": -1}),
            json!({"x": 1.5}),
            json!({"x": "1"}),
            json!({"x": null}),
            json!({}),
        ] {
            let (code, _, details) = u32_param(&params, "x").unwrap_err();
            assert_eq!(code, "invalid_params", "{params}");
            assert_eq!(details, params);
        }
    }

    #[test]
    fn bool_param_defaults_and_rejects_non_booleans() {
        assert!(bool_param(&json!({}), "all", true).unwrap());
        assert!(!bool_param(&json!({"all": null}), "all", false).unwrap());
        assert!(bool_param(&json!({"all": true}), "all", false).unwrap());
        assert!(!bool_param(&json!({"all": false}), "all", true).unwrap());
        for value in [json!(1), json!("true"), json!([])] {
            let (code, _, details) = bool_param(&json!({"all": value}), "all", false).unwrap_err();
            assert_eq!(code, "invalid_params");
            assert_eq!(details, value);
        }
    }

    #[test]
    fn button_param_names_and_default() {
        assert_eq!(button_param(&json!({})).unwrap(), MOUSE_BUTTON_LEFT);
        assert_eq!(
            button_param(&json!({"button": "left"})).unwrap(),
            MOUSE_BUTTON_LEFT
        );
        assert_eq!(
            button_param(&json!({"button": "right"})).unwrap(),
            MOUSE_BUTTON_RIGHT
        );
        assert_eq!(
            button_param(&json!({"button": "middle"})).unwrap(),
            MOUSE_BUTTON_MIDDLE
        );
        let (code, _, _) = button_param(&json!({"button": "back"})).unwrap_err();
        assert_eq!(code, "invalid_params");
    }

    #[test]
    fn key_code_names_single_characters_and_unknowns() {
        assert_eq!(key_code("Enter"), Some(KEY_ENTER));
        assert_eq!(key_code("return"), Some(KEY_ENTER));
        assert_eq!(key_code("ESC"), Some(KEY_ESCAPE));
        assert_eq!(key_code("PageDown"), Some(KEY_PAGE_DOWN));
        // A single character is its own code, lowercased like the names.
        assert_eq!(key_code("a"), Some('a' as u32));
        assert_eq!(key_code("A"), Some('a' as u32));
        assert_eq!(key_code("\u{3042}"), Some(0x3042));
        assert_eq!(key_code(""), None);
        assert_eq!(key_code("f13"), None);
    }
}
