use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::instruction::mnemonic;

use super::Machine;

impl Machine {
    // Thin forwarders the CPU load/store paths call. Cheap no-ops when profiling
    // is off, so they can sit unconditionally on the data-access path.
    pub(super) fn profile_mem_read(&mut self, address: u32) {
        if let Some(profiler) = self.profiler.as_mut() {
            profiler.record_mem_read(address);
        }
    }

    pub(super) fn profile_mem_write(&mut self, address: u32) {
        if let Some(profiler) = self.profiler.as_mut() {
            profiler.record_mem_write(address);
        }
    }
}

// Memory accesses are bucketed into pages so the heatmap stays compact even for
// programs that touch a large address range. 4 KB matches a conventional page.
const MEM_PAGE_SHIFT: u32 = 12;
const MEM_PAGE_SIZE: u32 = 1 << MEM_PAGE_SHIFT;

// One frame on the emulated call stack. Recorded when a CALL executes so a
// matching return can attribute inclusive time and pop back to the caller.
struct CallFrame {
    // Entry PC of the function being executed in this frame.
    func_entry: u32,
    // Address the function is expected to return to (the CALL's return address).
    // A `mov pc, lr` (or any jump) landing here is treated as this frame's return.
    return_addr: u32,
    // Instructions executed while this frame (and nothing it called) was on top.
    self_count: u64,
    // Instructions executed while this frame was anywhere on the stack, i.e.
    // including everything it called. Used for the inclusive column.
    inclusive_count: u64,
}

// Per-function aggregate accumulated across every activation of the function.
#[derive(Default, Clone)]
struct FuncStats {
    self_count: u64,
    inclusive_count: u64,
    call_count: u64,
}

// The Profiler is optional state hung off the Machine. When present, the run
// loop feeds it one event per executed instruction; on shutdown it serializes
// everything to a JSON file that qa/profile_report.py turns into a report.
pub struct Profiler {
    // Total instructions retired. The denominator for every percentage.
    total_instructions: u64,

    // Hotspots: self instruction count per PC.
    pc_hits: HashMap<u32, u64>,

    // Opcode histogram: index is the 6-bit opcode.
    opcode_hits: [u64; 64],

    // Memory heatmap: read/write counts per page base address.
    mem_reads: HashMap<u32, u64>,
    mem_writes: HashMap<u32, u64>,

    // Call graph.
    call_stack: Vec<CallFrame>,
    func_stats: HashMap<u32, FuncStats>,
    // Edge (caller_entry, callee_entry) -> number of calls along it.
    edges: HashMap<(u32, u32), u64>,
    // Entry PC of the program's outermost frame, so instructions retired before
    // the first CALL are still attributed somewhere sensible.
    root_entry: u32,
}

impl Profiler {
    pub fn new(entry_pc: u32) -> Self {
        let mut profiler = Profiler {
            total_instructions: 0,
            pc_hits: HashMap::new(),
            opcode_hits: [0; 64],
            mem_reads: HashMap::new(),
            mem_writes: HashMap::new(),
            call_stack: Vec::new(),
            func_stats: HashMap::new(),
            edges: HashMap::new(),
            root_entry: entry_pc,
        };
        // Seed the root frame. Its return address is unreachable (the program
        // halts rather than returning from it), so it only pops at shutdown.
        profiler.call_stack.push(CallFrame {
            func_entry: entry_pc,
            return_addr: u32::MAX,
            self_count: 0,
            inclusive_count: 0,
        });
        profiler.func_stats.entry(entry_pc).or_default().call_count = 1;
        profiler
    }

    // Called once per retired instruction, before the CALL/return bookkeeping.
    // `pc` is the address the instruction was fetched from and `opcode` its
    // decoded opcode. Charges self time to the current top-of-stack frame and
    // inclusive time to every frame.
    pub fn record_instruction(&mut self, pc: u32, opcode: u8) {
        self.total_instructions += 1;
        *self.pc_hits.entry(pc).or_insert(0) += 1;
        self.opcode_hits[(opcode & 0x3F) as usize] += 1;

        if let Some(top) = self.call_stack.last_mut() {
            top.self_count += 1;
        }
        for frame in &mut self.call_stack {
            frame.inclusive_count += 1;
        }
    }

    pub fn record_mem_read(&mut self, address: u32) {
        let page = address & !(MEM_PAGE_SIZE - 1);
        *self.mem_reads.entry(page).or_insert(0) += 1;
    }

    pub fn record_mem_write(&mut self, address: u32) {
        let page = address & !(MEM_PAGE_SIZE - 1);
        *self.mem_writes.entry(page).or_insert(0) += 1;
    }

    // A CALL just executed. `callee_entry` is the PC it jumped to; `return_addr`
    // is where control resumes when the callee returns (LR at call time). Pushes
    // a frame and records the call edge.
    pub fn record_call(&mut self, callee_entry: u32, return_addr: u32) {
        let caller_entry = self
            .call_stack
            .last()
            .map(|f| f.func_entry)
            .unwrap_or(self.root_entry);

        self.call_stack.push(CallFrame {
            func_entry: callee_entry,
            return_addr,
            self_count: 0,
            inclusive_count: 0,
        });

        self.func_stats.entry(callee_entry).or_default().call_count += 1;
        *self.edges.entry((caller_entry, callee_entry)).or_insert(0) += 1;
    }

    // The PC just changed to `new_pc` via something other than a CALL (a return,
    // a jump, or fallthrough). If it matches the return address recorded for the
    // current frame, treat it as that frame returning and fold its counts into
    // the aggregate. A while-loop handles tail chains that unwind several frames.
    pub fn record_control_flow(&mut self, new_pc: u32) {
        while self.call_stack.len() > 1 {
            let matches_return = self
                .call_stack
                .last()
                .map(|f| f.return_addr == new_pc)
                .unwrap_or(false);
            if !matches_return {
                break;
            }
            self.pop_frame();
        }
    }

    fn pop_frame(&mut self) {
        if let Some(frame) = self.call_stack.pop() {
            let stats = self.func_stats.entry(frame.func_entry).or_default();
            stats.self_count += frame.self_count;
            stats.inclusive_count += frame.inclusive_count;
        }
    }

    // Fold any frames still on the stack (the root, plus anything that halted
    // without returning) into the aggregate before serialization.
    fn flush_stack(&mut self) {
        while !self.call_stack.is_empty() {
            self.pop_frame();
        }
    }

    // Serialize everything to a JSON file. Hand-rolled so the emulator keeps a
    // zero-extra-dependency build; qa/profile_report.py consumes this.
    pub fn write_json<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        self.flush_stack();

        let file = File::create(path.as_ref()).map_err(|e| {
            format!(
                "Unable to open profile file {}: {}",
                path.as_ref().display(),
                e
            )
        })?;
        let mut out = BufWriter::new(file);

        writeln!(out, "{{").map_err(err)?;
        writeln!(
            out,
            "  \"total_instructions\": {},",
            self.total_instructions
        )
        .map_err(err)?;
        writeln!(out, "  \"root_entry\": {},", self.root_entry).map_err(err)?;

        // Hotspots, sorted by hit count descending for readability.
        let mut pc_pairs: Vec<(&u32, &u64)> = self.pc_hits.iter().collect();
        pc_pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        writeln!(out, "  \"pc_hits\": [").map_err(err)?;
        write_pairs(&mut out, &pc_pairs, |o, (addr, hits)| {
            write!(o, "    {{\"pc\": {}, \"hits\": {}}}", addr, hits)
        })?;
        writeln!(out, "  ],").map_err(err)?;

        // Opcode histogram: only non-zero opcodes, with mnemonics for convenience.
        let mut opcode_pairs: Vec<(usize, u64)> = self
            .opcode_hits
            .iter()
            .enumerate()
            .filter(|(_, &count)| count > 0)
            .map(|(op, &count)| (op, count))
            .collect();
        opcode_pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        writeln!(out, "  \"opcode_hits\": [").map_err(err)?;
        write_pairs(&mut out, &opcode_pairs, |o, (op, count)| {
            write!(
                o,
                "    {{\"opcode\": {}, \"mnemonic\": \"{}\", \"count\": {}}}",
                op,
                mnemonic(*op as u8),
                count
            )
        })?;
        writeln!(out, "  ],").map_err(err)?;

        // Memory heatmap.
        let mut read_pairs: Vec<(&u32, &u64)> = self.mem_reads.iter().collect();
        read_pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        writeln!(out, "  \"mem_reads\": [").map_err(err)?;
        write_pairs(&mut out, &read_pairs, |o, (page, count)| {
            write!(o, "    {{\"page\": {}, \"count\": {}}}", page, count)
        })?;
        writeln!(out, "  ],").map_err(err)?;

        let mut write_pairs_vec: Vec<(&u32, &u64)> = self.mem_writes.iter().collect();
        write_pairs_vec.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        writeln!(out, "  \"mem_writes\": [").map_err(err)?;
        write_pairs(&mut out, &write_pairs_vec, |o, (page, count)| {
            write!(o, "    {{\"page\": {}, \"count\": {}}}", page, count)
        })?;
        writeln!(out, "  ],").map_err(err)?;

        // Call graph: per-function aggregates.
        let mut func_pairs: Vec<(&u32, &FuncStats)> = self.func_stats.iter().collect();
        func_pairs.sort_by(|a, b| b.1.self_count.cmp(&a.1.self_count).then(a.0.cmp(b.0)));
        writeln!(out, "  \"functions\": [").map_err(err)?;
        write_pairs(&mut out, &func_pairs, |o, (entry, stats)| {
            write!(
                o,
                "    {{\"entry\": {}, \"self\": {}, \"inclusive\": {}, \"calls\": {}}}",
                entry, stats.self_count, stats.inclusive_count, stats.call_count
            )
        })?;
        writeln!(out, "  ],").map_err(err)?;

        // Call graph edges.
        let mut edge_pairs: Vec<(&(u32, u32), &u64)> = self.edges.iter().collect();
        edge_pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        writeln!(out, "  \"edges\": [").map_err(err)?;
        write_pairs(&mut out, &edge_pairs, |o, ((caller, callee), count)| {
            write!(
                o,
                "    {{\"caller\": {}, \"callee\": {}, \"count\": {}}}",
                caller, callee, count
            )
        })?;
        writeln!(out, "  ]").map_err(err)?;

        writeln!(out, "}}").map_err(err)?;
        out.flush().map_err(err)?;
        Ok(())
    }
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// Write a comma-separated JSON array body: `render` emits one element (without a
// trailing comma), and this inserts the separators and newlines between them.
fn write_pairs<W, T, F>(out: &mut W, items: &[T], mut render: F) -> Result<(), String>
where
    W: Write,
    F: FnMut(&mut W, &T) -> std::io::Result<()>,
{
    for (i, item) in items.iter().enumerate() {
        render(out, item).map_err(err)?;
        if i + 1 < items.len() {
            writeln!(out, ",").map_err(err)?;
        } else {
            writeln!(out).map_err(err)?;
        }
    }
    Ok(())
}
