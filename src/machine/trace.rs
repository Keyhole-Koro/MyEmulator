//! An ordered log of what ran, for replaying one stretch of a running system:
//! entries to and exits from watched functions, interrupts with their cause,
//! the returns from them, thread switches, and the control requests that let
//! the guest run. The profiler feeds it from its call stacks, so a trace sees
//! exactly the calls and returns the profile does.
//!
//! An event is stamped with the instructions retired so far, which orders
//! events exactly, and with the timer's clock (ticks << 8 plus how far the
//! current tick has got), the unit the kernel charges CPU time in.
use std::collections::HashSet;
use std::io::Write;

use serde_json::{json, Value};

/// What a trace records, given by trace.start.
#[derive(Clone)]
pub struct TraceOptions {
    /// Function entry PCs watched in any address space.
    pub pcs: HashSet<u32>,
    /// Function entry PCs watched in one address space only: (space, pc).
    pub spaced: HashSet<(u32, u32)>,
    /// Every call, whatever the lists say.
    pub all: bool,
    /// Interrupts, the returns from them, and thread switches.
    pub interrupts: bool,
    /// The most events kept. The log stops there and says it was cut short.
    pub limit: usize,
}

impl Default for TraceOptions {
    fn default() -> Self {
        Self {
            pcs: HashSet::new(),
            spaced: HashSet::new(),
            all: false,
            interrupts: true,
            limit: 1_000_000,
        }
    }
}

impl TraceOptions {
    pub fn watches(&self, space: u32, pc: u32) -> bool {
        self.all || self.pcs.contains(&pc) || self.spaced.contains(&(space, pc))
    }
}

/// Why an interrupt vectored: the cause bits pending when it did, the trap
/// among them if an instruction raised one, and for a syscall its number (r1)
/// and first argument (r5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IrqInfo {
    /// The address space the interrupted code ran in.
    pub space: u32,
    pub cause: u32,
    pub trap: u32,
    pub syscall: Option<(u32, u32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceKind {
    /// A watched function was called. `pc` is its entry.
    Enter,
    /// It returned. Unwound: a frame below it returned first, so it ended
    /// without a return of its own, as frames a thread switch left do.
    Exit { unwound: bool },
    /// An interrupt or trap vectored. `pc` is the vector.
    Irq(IrqInfo),
    /// The interrupted code resumed.
    Iret { unwound: bool },
    /// The kernel's current-thread word changed; `thread` is the new value.
    Switch { from: u32 },
    /// A control request started, and with it the guest ran again.
    Request { method: String },
}

#[derive(Clone, Debug)]
pub struct TraceEvent {
    pub retired: u64,
    pub clock: u32,
    pub thread: u32,
    pub space: u32,
    pub pc: u32,
    pub kind: TraceKind,
}

/// One thread's call stack when the trace started, outermost frame first:
/// each frame's space, entry PC and the address it returns to, which names
/// its caller even when the caller was entered before the profile saw it.
pub struct ThreadStack {
    pub thread: u32,
    pub frames: Vec<(u32, u32, u32)>,
}

/// An address space the events ran in, with the text a reader matches against
/// the built images to name its program.
pub struct SpaceText<'a> {
    pub space: u32,
    pub text_base: u32,
    pub text_sample: &'a [u8],
}

pub struct EventLog {
    pub options: TraceOptions,
    events: Vec<TraceEvent>,
    // Events before this one carry their stamp; the machine stamps the rest.
    stamped: usize,
    recording: bool,
    truncated: bool,
    started: (u64, u32),
    stacks: Vec<ThreadStack>,
}

impl EventLog {
    pub fn new(options: TraceOptions, stacks: Vec<ThreadStack>) -> Self {
        Self {
            options,
            events: Vec::new(),
            stamped: 0,
            recording: true,
            truncated: false,
            started: (0, 0),
            stacks,
        }
    }

    pub fn recording(&self) -> bool {
        self.recording
    }

    pub fn stop(&mut self) {
        self.recording = false;
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    #[cfg(test)]
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    pub fn set_started(&mut self, retired: u64, clock: u32) {
        self.started = (retired, clock);
    }

    /// Appends an event, stamped later. False once the log is full or stopped.
    pub fn push(&mut self, thread: u32, space: u32, pc: u32, kind: TraceKind) -> bool {
        if !self.recording {
            return false;
        }
        if self.events.len() >= self.options.limit {
            self.truncated = true;
            self.recording = false;
            return false;
        }
        self.events.push(TraceEvent {
            retired: 0,
            clock: 0,
            thread,
            space,
            pc,
            kind,
        });
        true
    }

    pub fn unstamped(&self) -> bool {
        self.stamped < self.events.len()
    }

    pub fn stamp(&mut self, retired: u64, clock: u32) {
        for event in &mut self.events[self.stamped..] {
            event.retired = retired;
            event.clock = clock;
        }
        self.stamped = self.events.len();
    }

    /// The spaces the stacks and the events from `since` on ran in, each once.
    pub fn spaces_from(&self, since: usize) -> Vec<u32> {
        let mut seen = Vec::new();
        let frames = self
            .stacks
            .iter()
            .flat_map(|stack| stack.frames.iter().map(|frame| frame.0));
        let events = self.events.iter().skip(since).map(|event| event.space);
        for space in frames.chain(events) {
            if !seen.contains(&space) {
                seen.push(space);
            }
        }
        seen
    }

    fn header(&self, spaces: &[SpaceText]) -> Vec<(&'static str, Value)> {
        let options = &self.options;
        let mut watched: Vec<Value> = options.pcs.iter().map(|pc| json!({"pc": pc})).collect();
        watched.extend(
            options
                .spaced
                .iter()
                .map(|(space, pc)| json!({"pc": pc, "space": space})),
        );
        watched.sort_by_key(|value| (value["pc"].as_u64(), value["space"].as_u64()));
        vec![
            ("format", json!(1)),
            (
                "started",
                json!({"retired": self.started.0, "clock": self.started.1}),
            ),
            ("recording", json!(self.recording)),
            ("truncated", json!(self.truncated)),
            (
                "options",
                json!({
                    "watch": watched,
                    "all": options.all,
                    "interrupts": options.interrupts,
                    "limit": options.limit,
                }),
            ),
            (
                "stacks",
                Value::Array(self.stacks.iter().map(stack_value).collect()),
            ),
            (
                "spaces",
                Value::Array(spaces.iter().map(space_value).collect()),
            ),
        ]
    }

    /// The log as one JSON value, holding the events from `since` on.
    pub fn to_value(&self, since: usize, spaces: &[SpaceText]) -> Value {
        let mut object = serde_json::Map::new();
        for (key, value) in self.header(spaces) {
            object.insert(key.to_string(), value);
        }
        object.insert("next".to_string(), json!(self.events.len()));
        let events = self.events.iter().skip(since).map(event_value).collect();
        object.insert("events".to_string(), Value::Array(events));
        Value::Object(object)
    }

    /// The same JSON as `to_value(0, ..)`, written an event a line, so a long
    /// log never has to be held as one value.
    pub fn write_to<W: Write>(&self, out: &mut W, spaces: &[SpaceText]) -> std::io::Result<()> {
        writeln!(out, "{{")?;
        for (key, value) in self.header(spaces) {
            writeln!(out, "  \"{key}\": {value},")?;
        }
        writeln!(out, "  \"next\": {},", self.events.len())?;
        writeln!(out, "  \"events\": [")?;
        for (i, event) in self.events.iter().enumerate() {
            let separator = if i + 1 < self.events.len() { "," } else { "" };
            writeln!(out, "    {}{separator}", event_value(event))?;
        }
        writeln!(out, "  ]")?;
        writeln!(out, "}}")
    }
}

// Thread ids are the kernel's word, read signed: -1 is none running, -2 is
// before the kernel first wrote it.
fn thread_id(thread: u32) -> i64 {
    i64::from(thread as i32)
}

fn event_value(event: &TraceEvent) -> Value {
    let mut value = json!({
        "retired": event.retired,
        "clock": event.clock,
        "thread": thread_id(event.thread),
        "space": event.space,
    });
    let fields = value.as_object_mut().expect("an object");
    // A switch or a request happens at no function.
    if !matches!(
        event.kind,
        TraceKind::Switch { .. } | TraceKind::Request { .. }
    ) {
        fields.insert("pc".to_string(), json!(event.pc));
    }
    let kind = match &event.kind {
        TraceKind::Enter => "enter",
        TraceKind::Exit { unwound } => {
            if *unwound {
                fields.insert("unwound".to_string(), json!(true));
            }
            "exit"
        }
        TraceKind::Irq(info) => {
            fields.insert("cause".to_string(), json!(info.cause));
            fields.insert("trap".to_string(), json!(info.trap));
            if let Some((number, argument)) = info.syscall {
                fields.insert("syscall".to_string(), json!(number));
                fields.insert("arg".to_string(), json!(argument));
            }
            "irq"
        }
        TraceKind::Iret { unwound } => {
            if *unwound {
                fields.insert("unwound".to_string(), json!(true));
            }
            "iret"
        }
        TraceKind::Switch { from } => {
            fields.insert("from".to_string(), json!(thread_id(*from)));
            "switch"
        }
        TraceKind::Request { method } => {
            fields.insert("method".to_string(), json!(method));
            "request"
        }
    };
    fields.insert("kind".to_string(), json!(kind));
    value
}

fn stack_value(stack: &ThreadStack) -> Value {
    // A root frame returns nowhere.
    let frames: Vec<Value> = stack
        .frames
        .iter()
        .map(|&(space, pc, ret)| {
            let ret = (ret != u32::MAX).then_some(ret);
            json!({"space": space, "pc": pc, "return": ret})
        })
        .collect();
    json!({"thread": thread_id(stack.thread), "frames": frames})
}

fn space_value(space: &SpaceText) -> Value {
    let sample: String = space
        .text_sample
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"space": space.space, "text_base": space.text_base, "text_sample": sample})
}
