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
use crate::machine::Machine;

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

fn handle(machine: &mut Machine, request: Request) {
    let result = match request.method.as_str() {
        "session.hello" => Ok(json!({
            "protocol": 2,
            "capabilities": ["dom.snapshot", "dom.hit_test", "input.pointer.sequence", "input.key.type", "input.key.press", "screen.screenshot"]
        })),
        "os.ready" => wait_for_dom(machine).map(|_| json!({"ready": true})),
        "dom.snapshot" => dom_snapshot(machine),
        "dom.hit_test" => dom_hit_test(machine, &request.params),
        "input.pointer.sequence" => pointer_sequence(machine, &request.params),
        "input.key.type" => key_type(machine, &request.params),
        "input.key.press" => key_press(machine, &request.params),
        "screen.screenshot" => screenshot(machine, &request.params),
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

fn screenshot(machine: &mut Machine, params: &Value) -> Result<Value, (&'static str, String, Value)> {
    let path = params.get("path").and_then(Value::as_str)
        .ok_or_else(|| ("invalid_params", "path must be a string".to_string(), Value::Null))?;
    machine.write_screenshot(path).map_err(|e| ("screenshot_failed", e, Value::Null))?;
    Ok(json!({"path": path}))
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
    value.get(key).and_then(Value::as_u64).map(|v| v as u32)
        .ok_or_else(|| ("invalid_params", format!("{} must be an unsigned integer", key), value.clone()))
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
