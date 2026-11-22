//! The frame timeline: when things happened, on the wall clock, with the
//! guest's instruction count beside each event.
//!
//! The guest marks the stages of its work through three MMIO registers
//! (TIMELINE_BEGIN / END / MARK, a name's address after its length in
//! TIMELINE_LENGTH): the compositor's pass, input, paint and so on. The
//! emulator adds its own work on a second track -- presenting frames, pumping
//! the window, sleeping on WFI -- and a third for what the screen got: guest
//! swaps and cursor moves. The result is a Chrome trace (JSON, "Trace Event
//! Format") that https://ui.perfetto.dev opens, so a slow frame shows what it
//! spent its time on and what the emulator was doing meanwhile.
//!
//! Recording costs nothing until it is started (`--timeline`, or the control
//! request `timeline.start`): the registers are ignored while it is off.
use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// Tracks ("threads" in the trace).
pub(super) const GUEST: u32 = 1;
pub(super) const EMULATOR: u32 = 2;
pub(super) const SCREEN: u32 = 3;
pub(super) const DMA: u32 = 4;

// A recording longer than this keeps its first events and counts the rest.
const MAX_EVENTS: usize = 4_000_000;
// How often the guest's speed is sampled onto its counter track.
const COUNTER_PERIOD: Duration = Duration::from_millis(100);
// The longest marker name read out of guest memory.
pub(super) const MAX_NAME: u32 = 64;

enum Phase {
    Begin,
    End,
    Instant,
    // A span whose length is known when it is recorded.
    Complete(Duration),
    Counter(f64),
}

struct Event {
    phase: Phase,
    track: u32,
    at: Duration,
    name: String,
    instrs: u64,
}

pub(super) struct Timeline {
    start: Instant,
    events: Vec<Event>,
    dropped: u64,
    // TIMELINE_LENGTH: the length of the name the next marker write names.
    pub name_length: u32,
    counter_at: Instant,
    counter_instrs: u64,
}

impl Timeline {
    pub fn new(instrs: u64) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            events: Vec::new(),
            dropped: 0,
            name_length: 0,
            counter_at: now,
            counter_instrs: instrs,
        }
    }

    fn push(&mut self, phase: Phase, track: u32, at: Instant, name: &str, instrs: u64) {
        if self.events.len() >= MAX_EVENTS {
            self.dropped += 1;
            return;
        }
        self.events.push(Event {
            phase,
            track,
            at: at.saturating_duration_since(self.start),
            name: name.to_string(),
            instrs,
        });
    }

    pub fn begin(&mut self, track: u32, name: &str, instrs: u64) {
        self.push(Phase::Begin, track, Instant::now(), name, instrs);
    }

    pub fn end(&mut self, track: u32, name: &str, instrs: u64) {
        self.push(Phase::End, track, Instant::now(), name, instrs);
    }

    pub fn instant(&mut self, track: u32, name: &str, instrs: u64) {
        self.push(Phase::Instant, track, Instant::now(), name, instrs);
    }

    /// A span that began at `since` and ends now.
    pub fn span(&mut self, track: u32, name: &str, since: Instant, instrs: u64) {
        let length = since.elapsed();
        self.push(Phase::Complete(length), track, since, name, instrs);
    }

    /// Samples the guest's speed when a period has passed since the last.
    pub fn sample_speed(&mut self, instrs: u64) {
        let elapsed = self.counter_at.elapsed();
        if elapsed < COUNTER_PERIOD {
            return;
        }
        let rate = instrs.saturating_sub(self.counter_instrs) as f64 / elapsed.as_secs_f64() / 1e6;
        let at = self.counter_at;
        self.counter_at = Instant::now();
        self.counter_instrs = instrs;
        self.push(Phase::Counter(rate), GUEST, at, "guest M instrs/s", instrs);
    }

    pub fn write(&self, path: &str) -> Result<(), String> {
        let mut out = String::with_capacity(self.events.len() * 96 + 512);
        out.push_str("{\"displayTimeUnit\":\"ms\",\"traceEvents\":[\n");
        let names = [(GUEST, "guest CPU"), (EMULATOR, "emulator (host)"), (SCREEN, "screen"), (DMA, "2D DMA (host)")];
        out.push_str("{\"name\":\"process_name\",\"ph\":\"M\",\"pid\":1,\"args\":{\"name\":\"MyComputer\"}}");
        for (track, name) in names {
            let _ = write!(
                out,
                ",\n{{\"name\":\"thread_name\",\"ph\":\"M\",\"pid\":1,\"tid\":{track},\"args\":{{\"name\":\"{name}\"}}}}"
            );
            let _ = write!(
                out,
                ",\n{{\"name\":\"thread_sort_index\",\"ph\":\"M\",\"pid\":1,\"tid\":{track},\"args\":{{\"sort_index\":{track}}}}}"
            );
        }
        for event in &self.events {
            let ts = event.at.as_nanos() as f64 / 1000.0;
            out.push_str(",\n{\"name\":");
            push_json_string(&mut out, &event.name);
            let _ = write!(out, ",\"pid\":1,\"tid\":{},\"ts\":{ts:.3}", event.track);
            match event.phase {
                Phase::Begin => out.push_str(",\"ph\":\"B\""),
                Phase::End => out.push_str(",\"ph\":\"E\""),
                Phase::Instant => out.push_str(",\"ph\":\"i\",\"s\":\"t\""),
                Phase::Complete(length) => {
                    let _ = write!(out, ",\"ph\":\"X\",\"dur\":{:.3}", length.as_nanos() as f64 / 1000.0);
                }
                Phase::Counter(value) => {
                    let _ = write!(out, ",\"ph\":\"C\",\"args\":{{\"M/s\":{value:.3}}}}}");
                    continue;
                }
            }
            let _ = write!(out, ",\"args\":{{\"instrs\":{}}}}}", event.instrs);
        }
        let _ = write!(
            out,
            "\n],\"otherData\":{{\"events\":{},\"dropped\":{}}}}}\n",
            self.events.len(),
            self.dropped
        );
        std::fs::write(path, out).map_err(|e| format!("cannot write timeline {path}: {e}"))
    }
}

fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timeline_is_a_chrome_trace() {
        let mut timeline = Timeline::new(0);
        timeline.begin(GUEST, "paint \"all\"", 10);
        timeline.instant(SCREEN, "swap", 20);
        timeline.end(GUEST, "paint \"all\"", 30);
        timeline.span(EMULATOR, "present", Instant::now(), 30);
        let path = std::env::temp_dir().join(format!("timeline-test-{}.json", std::process::id()));
        timeline.write(path.to_str().unwrap()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let events = value["traceEvents"].as_array().unwrap();
        let phases: Vec<&str> = events.iter().filter_map(|e| e["ph"].as_str()).filter(|p| *p != "M").collect();
        assert_eq!(phases, ["B", "i", "E", "X"]);
        assert_eq!(events.iter().find(|e| e["ph"] == "B").unwrap()["name"], "paint \"all\"");
        assert_eq!(events.iter().find(|e| e["ph"] == "E").unwrap()["args"]["instrs"], 30);
    }
}
