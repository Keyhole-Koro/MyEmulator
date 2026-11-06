use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::instruction::mnemonic;

use super::Machine;

impl Machine {
    // The address space an instruction runs in: one user process, or 0 for the
    // kernel and for programs without an MMU. User processes share virtual
    // addresses, so a PC alone does not name a function.
    pub(super) fn profile_space(&self) -> u32 {
        match &self.execution.profiler {
            Some(profiler) if self.is_user_mode() => profiler.user_space(),
            _ => 0,
        }
    }

    // The kernel loaded a page directory. A directory freed by an exited process
    // can come back holding another program, so look at its text again.
    pub(super) fn profile_pdbr_written(&mut self, pdbr: u32) {
        let Some(profiler) = self.execution.profiler.as_ref() else {
            return;
        };
        let sample = profiler
            .text_base(pdbr)
            .map(|base| self.profile_text_sample(base));
        if let Some(profiler) = self.execution.profiler.as_mut() {
            profiler.enter_space(pdbr, sample.as_deref());
        }
    }

    // Thin forwarders the CPU load/store paths call. Cheap no-ops when profiling
    // is off, so they can sit unconditionally on the data-access path.
    pub(super) fn profile_mem_read(&mut self, address: u32) {
        if let Some(profiler) = self.execution.profiler.as_mut() {
            profiler.record_mem_read(address);
        }
    }

    pub(super) fn profile_mem_write(&mut self, address: u32) {
        if let Some(profiler) = self.execution.profiler.as_mut() {
            profiler.record_mem_write(address);
        }
    }

    // A word store: the one write that can change the current thread.
    pub(super) fn profile_word_write(&mut self, address: u32, value: u32) {
        let user = self.is_user_mode();
        if let Some(profiler) = self.execution.profiler.as_mut() {
            profiler.record_mem_write(address);
            if !user {
                profiler.record_word_store(address, value);
            }
        }
    }

    // An interrupt or trap vectored: from here to the matching `iret` is a call
    // the program never wrote. `return_pc` and `return_sp` are what `iret` restores.
    pub(super) fn profile_interrupt(&mut self, vector: u32, return_pc: u32, return_sp: u32) {
        if let Some(profiler) = self.execution.profiler.as_mut() {
            profiler.record_interrupt(vector, return_pc, return_sp);
        }
    }

    // Read the text page at `base`, without touching accessed bits or the TLB. A report matches these bytes against the built images to
    // learn which program ran in an address space.
    pub(super) fn profile_text_sample(&self, base: u32) -> Vec<u8> {
        let mut sample = Vec::with_capacity(TEXT_SAMPLE_BYTES as usize);
        let ram = self.memory.ram.as_slice();
        if let Some(physical) = self.memory.mmu.peek(ram, base) {
            let start = physical as usize;
            let end = (start + TEXT_SAMPLE_BYTES as usize).min(ram.len());
            if start < end {
                sample.extend_from_slice(&ram[start..end]);
            }
        }
        sample
    }
}

// Memory accesses are bucketed into pages so the heatmap stays compact even for
// programs that touch a large address range. 4 KB matches a conventional page.
const MEM_PAGE_SHIFT: u32 = 12;
const MEM_PAGE_SIZE: u32 = 1 << MEM_PAGE_SHIFT;

// One page: enough to tell programs apart, and no read crosses a mapping.
const TEXT_SAMPLE_BYTES: u32 = 4096;

// How far down the stack a jump looks for the frame it returns to. Interrupted
// frames can sit under a few that a thread switch left behind.
const RETURN_SEARCH_DEPTH: usize = 64;

// A function: its address space and entry PC.
type FuncKey = (u32, u32);

// One frame on the emulated call stack. Recorded when a CALL executes so a
// matching return can attribute inclusive time and pop back to the caller.
struct CallFrame {
    func: FuncKey,
    // Where the function returns to, and the SP it returns with. A jump to the
    // address with SP below this is not the return: an inner call of a recursive
    // function can branch to the instruction after its own call site.
    return_addr: u32,
    return_sp: u32,
    // Instructions executed while this frame was on top.
    self_count: u64,
    // The thread's instruction count when the frame was pushed. The frame's
    // inclusive count is how far that count has moved since.
    entered_at: u64,
    // False for a recursive call, so inclusive time counts once per function.
    outermost: bool,
}

// A call stack and instruction count for one thread of execution.
#[derive(Default)]
struct ThreadState {
    frames: Vec<CallFrame>,
    retired: u64,
}

impl ThreadState {
    fn push(&mut self, func: FuncKey, return_addr: u32, return_sp: u32) {
        let outermost = !self.frames.iter().any(|frame| frame.func == func);
        self.frames.push(CallFrame {
            func,
            return_addr,
            return_sp,
            self_count: 0,
            entered_at: self.retired,
            outermost,
        });
    }
}

// Per-function aggregate accumulated across every activation of the function.
#[derive(Default, Clone)]
struct FuncStats {
    self_count: u64,
    inclusive_count: u64,
    call_count: u64,
}

impl FuncStats {
    fn fold(&mut self, frame: &CallFrame, retired: u64) {
        self.self_count += frame.self_count;
        if frame.outermost {
            self.inclusive_count += retired - frame.entered_at;
        }
    }
}

// One address space the profile saw.
#[derive(Default)]
struct SpaceStats {
    instructions: u64,
    text_base: u32,
    text_sample: Vec<u8>,
}

// The Profiler is optional state hung off the Machine. When present, the run
// loop feeds it one event per executed instruction; on request it serializes
// everything to a JSON file that MyProfiler turns into a report.
pub struct Profiler {
    // Total instructions retired. The denominator for every percentage.
    total_instructions: u64,

    // Hotspots: self instruction count per PC.
    pc_hits: HashMap<FuncKey, u64>,

    // Opcode histogram: index is the 6-bit opcode.
    opcode_hits: [u64; 64],

    // Memory heatmap: read/write counts per page base address.
    mem_reads: HashMap<u32, u64>,
    mem_writes: HashMap<u32, u64>,

    // Call graph.
    func_stats: HashMap<FuncKey, FuncStats>,
    // Edge (caller, callee) -> number of calls along it.
    edges: HashMap<(FuncKey, FuncKey), u64>,

    // A space is its page directory's address plus, in the low bits a directory
    // address leaves free, how many programs that directory has held before.
    spaces: HashMap<u32, SpaceStats>,
    space_reuses: HashMap<u32, u32>,
    user_space: u32,
    last_space: Option<u32>,

    // The running thread and the ones switched out. Without a thread variable
    // there is only `current`, which suits a program with one thread.
    current: ThreadState,
    current_thread: u32,
    parked: HashMap<u32, ThreadState>,
    thread_var: Option<u32>,
    // Instructions per thread since the last reset.
    thread_hits: HashMap<u32, u64>,
}

// The thread id before the kernel first writes its variable.
const BOOT_THREAD: u32 = u32::MAX - 1;

impl Profiler {
    pub fn new(entry_pc: u32, thread_var: Option<u32>) -> Self {
        let mut profiler = Profiler {
            total_instructions: 0,
            pc_hits: HashMap::new(),
            opcode_hits: [0; 64],
            mem_reads: HashMap::new(),
            mem_writes: HashMap::new(),
            func_stats: HashMap::new(),
            edges: HashMap::new(),
            spaces: HashMap::new(),
            space_reuses: HashMap::new(),
            user_space: 0,
            last_space: None,
            current: ThreadState::default(),
            current_thread: BOOT_THREAD,
            parked: HashMap::new(),
            thread_var,
            thread_hits: HashMap::new(),
        };
        profiler.seed_root((0, entry_pc));
        profiler
    }

    // Instructions run before any CALL still need a frame to land in. A root
    // frame returns nowhere, so it stays until the end.
    fn seed_root(&mut self, func: FuncKey) {
        self.current.push(func, u32::MAX, u32::MAX);
        self.func_stats.entry(func).or_default().call_count += 1;
    }

    // Called once per retired instruction, before the CALL/return bookkeeping.
    // `pc` is the address the instruction was fetched from and `space` the
    // address space it ran in. Returns true the first time a space is seen, so
    // the caller can supply a text sample.
    pub fn record_instruction(&mut self, pc: u32, space: u32, opcode: u8) -> bool {
        self.total_instructions += 1;
        *self.pc_hits.entry((space, pc)).or_insert(0) += 1;
        self.opcode_hits[(opcode & 0x3F) as usize] += 1;
        *self.thread_hits.entry(self.current_thread).or_insert(0) += 1;

        if self.current.frames.is_empty() {
            self.seed_root((space, pc));
        }
        self.current.retired += 1;
        if let Some(top) = self.current.frames.last_mut() {
            top.self_count += 1;
        }

        let new_space = self.last_space != Some(space) && !self.spaces.contains_key(&space);
        self.last_space = Some(space);
        self.spaces.entry(space).or_default().instructions += 1;
        new_space
    }

    pub fn set_text_sample(&mut self, space: u32, base: u32, sample: Vec<u8>) {
        let stats = self.spaces.entry(space).or_default();
        stats.text_base = base;
        stats.text_sample = sample;
    }

    pub fn user_space(&self) -> u32 {
        self.user_space
    }

    fn space_of(&self, pdbr: u32) -> u32 {
        pdbr | self.space_reuses.get(&pdbr).copied().unwrap_or(0)
    }

    // Where to look to tell whether `pdbr` still holds the same program.
    pub fn text_base(&self, pdbr: u32) -> Option<u32> {
        self.spaces
            .get(&self.space_of(pdbr))
            .filter(|stats| !stats.text_sample.is_empty())
            .map(|stats| stats.text_base)
    }

    // `sample` is what the text looks like now, if the directory ran before.
    pub fn enter_space(&mut self, pdbr: u32, sample: Option<&[u8]>) {
        let space = self.space_of(pdbr);
        let same = match (sample, self.spaces.get(&space)) {
            (Some(sample), Some(stats)) => stats.text_sample == sample,
            _ => true,
        };
        if !same {
            let reuses = self.space_reuses.entry(pdbr).or_insert(0);
            *reuses = (*reuses + 1) & (MEM_PAGE_SIZE - 1);
        }
        self.user_space = self.space_of(pdbr);
    }

    pub fn record_mem_read(&mut self, address: u32) {
        let page = address & !(MEM_PAGE_SIZE - 1);
        *self.mem_reads.entry(page).or_insert(0) += 1;
    }

    pub fn record_mem_write(&mut self, address: u32) {
        let page = address & !(MEM_PAGE_SIZE - 1);
        *self.mem_writes.entry(page).or_insert(0) += 1;
    }

    // A kernel word store. A store to the thread variable switches call stacks:
    // the frames below belong to the thread going out and resume when it returns.
    pub fn record_word_store(&mut self, address: u32, value: u32) {
        if self.thread_var != Some(address) || value == self.current_thread {
            return;
        }
        let next = self.parked.remove(&value).unwrap_or_default();
        let previous = std::mem::replace(&mut self.current, next);
        self.parked.insert(self.current_thread, previous);
        self.current_thread = value;
    }

    // A CALL just executed. `callee_entry` is the PC it jumped to; `return_addr`
    // is where control resumes when the callee returns (LR at call time) and
    // `sp` the stack pointer then. Pushes a frame and records the call edge.
    pub fn record_call(&mut self, callee_entry: u32, space: u32, return_addr: u32, sp: u32) {
        self.enter((space, callee_entry), return_addr, sp);
    }

    pub fn record_interrupt(&mut self, vector: u32, return_addr: u32, sp: u32) {
        self.enter((0, vector), return_addr, sp);
    }

    fn enter(&mut self, callee: FuncKey, return_addr: u32, sp: u32) {
        let caller = self.current.frames.last().map(|frame| frame.func).unwrap_or(callee);
        self.current.push(callee, return_addr, sp);
        self.func_stats.entry(callee).or_default().call_count += 1;
        *self.edges.entry((caller, callee)).or_insert(0) += 1;
    }

    // Control jumped to `new_pc` with something other than a CALL. If that is
    // where a frame returns to, it and the frames above it are done: the ones
    // above are what a thread switch or a non-local exit left unfinished.
    pub fn record_jump(&mut self, new_pc: u32, sp: u32) {
        let frames = &self.current.frames;
        let lowest = frames.len().saturating_sub(RETURN_SEARCH_DEPTH).max(1);
        let target = (lowest..frames.len())
            .rev()
            .find(|&i| frames[i].return_addr == new_pc && sp >= frames[i].return_sp);
        if let Some(index) = target {
            while self.current.frames.len() > index {
                self.pop_frame();
            }
        }
    }

    fn pop_frame(&mut self) {
        if let Some(frame) = self.current.frames.pop() {
            self.func_stats
                .entry(frame.func)
                .or_default()
                .fold(&frame, self.current.retired);
        }
    }

    // Forget what was counted so far but keep the call stacks, so a later report
    // covers one stretch of a running system.
    pub fn reset(&mut self) {
        self.total_instructions = 0;
        self.pc_hits.clear();
        self.opcode_hits = [0; 64];
        self.mem_reads.clear();
        self.mem_writes.clear();
        self.func_stats.clear();
        self.edges.clear();
        self.thread_hits.clear();
        for stats in self.spaces.values_mut() {
            stats.instructions = 0;
        }
        for state in std::iter::once(&mut self.current).chain(self.parked.values_mut()) {
            for frame in &mut state.frames {
                frame.self_count = 0;
                frame.entered_at = state.retired;
            }
        }
    }

    // The aggregates plus whatever frames are still running, which leaves the
    // live stacks alone so a running system can be written more than once.
    fn function_totals(&self) -> HashMap<FuncKey, FuncStats> {
        let mut totals = self.func_stats.clone();
        for state in std::iter::once(&self.current).chain(self.parked.values()) {
            for frame in &state.frames {
                totals.entry(frame.func).or_default().fold(frame, state.retired);
            }
        }
        totals
    }

    // Serialize everything to a JSON file. Hand-rolled so the emulator keeps a
    // zero-extra-dependency build; MyProfiler documents and consumes the format.
    pub fn write_json<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let file = File::create(path.as_ref()).map_err(|e| {
            format!(
                "Unable to open profile file {}: {}",
                path.as_ref().display(),
                e
            )
        })?;
        let mut out = BufWriter::new(file);

        writeln!(out, "{{").map_err(err)?;
        writeln!(out, "  \"format\": 2,").map_err(err)?;
        writeln!(
            out,
            "  \"total_instructions\": {},",
            self.total_instructions
        )
        .map_err(err)?;

        // Address spaces, with the text sample a report identifies them by.
        let mut spaces: Vec<(&u32, &SpaceStats)> = self.spaces.iter().collect();
        spaces.sort_by(|a, b| b.1.instructions.cmp(&a.1.instructions).then(a.0.cmp(b.0)));
        writeln!(out, "  \"spaces\": [").map_err(err)?;
        write_pairs(&mut out, &spaces, |o, (space, stats)| {
            let sample: String = stats.text_sample.iter().map(|b| format!("{b:02x}")).collect();
            write!(
                o,
                "    {{\"space\": {}, \"instructions\": {}, \"text_sample\": \"{}\"}}",
                space, stats.instructions, sample
            )
        })?;
        writeln!(out, "  ],").map_err(err)?;

        // Threads. -1 as a signed id is the kernel's "none running".
        let mut threads: Vec<(&u32, &u64)> = self.thread_hits.iter().collect();
        threads.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        writeln!(out, "  \"threads\": [").map_err(err)?;
        write_pairs(&mut out, &threads, |o, (thread, count)| {
            write!(
                o,
                "    {{\"thread\": {}, \"instructions\": {}}}",
                **thread as i32, count
            )
        })?;
        writeln!(out, "  ],").map_err(err)?;

        // Hotspots, sorted by hit count descending for readability.
        let mut pc_pairs: Vec<(&FuncKey, &u64)> = self.pc_hits.iter().collect();
        pc_pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        writeln!(out, "  \"pc_hits\": [").map_err(err)?;
        write_pairs(&mut out, &pc_pairs, |o, ((space, pc), hits)| {
            write!(
                o,
                "    {{\"space\": {}, \"pc\": {}, \"hits\": {}}}",
                space, pc, hits
            )
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
        let totals = self.function_totals();
        let mut func_pairs: Vec<(&FuncKey, &FuncStats)> = totals.iter().collect();
        func_pairs.sort_by(|a, b| b.1.self_count.cmp(&a.1.self_count).then(a.0.cmp(b.0)));
        writeln!(out, "  \"functions\": [").map_err(err)?;
        write_pairs(&mut out, &func_pairs, |o, ((space, entry), stats)| {
            write!(
                o,
                "    {{\"space\": {}, \"entry\": {}, \"self\": {}, \"inclusive\": {}, \"calls\": {}}}",
                space, entry, stats.self_count, stats.inclusive_count, stats.call_count
            )
        })?;
        writeln!(out, "  ],").map_err(err)?;

        // Call graph edges.
        let mut edge_pairs: Vec<(&(FuncKey, FuncKey), &u64)> = self.edges.iter().collect();
        edge_pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        writeln!(out, "  \"edges\": [").map_err(err)?;
        write_pairs(&mut out, &edge_pairs, |o, (((from_space, caller), (to_space, callee)), count)| {
            write!(
                o,
                "    {{\"caller_space\": {}, \"caller\": {}, \"callee_space\": {}, \"callee\": {}, \"count\": {}}}",
                from_space, caller, to_space, callee, count
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

#[cfg(test)]
mod tests {
    use super::*;

    const SP: u32 = 0x1000;

    fn stats(profiler: &Profiler, entry: u32) -> FuncStats {
        profiler.function_totals().get(&(0, entry)).cloned().unwrap_or_default()
    }

    fn run(profiler: &mut Profiler, pc: u32, count: u32) {
        for i in 0..count {
            profiler.record_instruction(pc + i * 4, 0, 0);
        }
    }

    #[test]
    fn a_return_folds_self_and_inclusive_into_the_callee() {
        let mut p = Profiler::new(0x100, None);
        run(&mut p, 0x100, 2);
        p.record_call(0x200, 0, 0x108, SP);
        run(&mut p, 0x200, 3);
        p.record_jump(0x108, SP);
        run(&mut p, 0x108, 1);

        let callee = stats(&p, 0x200);
        assert_eq!((callee.self_count, callee.inclusive_count, callee.call_count), (3, 3, 1));
        let root = stats(&p, 0x100);
        assert_eq!((root.self_count, root.inclusive_count), (3, 6));
    }

    #[test]
    fn a_branch_to_a_return_address_below_the_frames_sp_is_not_a_return() {
        // The inner call of a recursive function skips its own call site and
        // lands on the address the outer call returns to, with SP still lower.
        let mut p = Profiler::new(0x100, None);
        p.record_call(0x200, 0, 0x108, SP);
        p.record_call(0x200, 0, 0x210, SP - 16);
        p.record_jump(0x210, SP - 32);
        assert_eq!(p.current.frames.len(), 3);
        p.record_jump(0x210, SP - 16);
        assert_eq!(p.current.frames.len(), 2);
    }

    #[test]
    fn recursion_counts_inclusive_time_once() {
        let mut p = Profiler::new(0x100, None);
        p.record_call(0x200, 0, 0x108, SP);
        run(&mut p, 0x200, 1);
        p.record_call(0x200, 0, 0x210, SP - 16);
        run(&mut p, 0x200, 1);
        p.record_jump(0x210, SP - 16);
        p.record_jump(0x108, SP);
        let f = stats(&p, 0x200);
        assert_eq!((f.self_count, f.inclusive_count, f.call_count), (2, 2, 2));
    }

    #[test]
    fn a_thread_switch_keeps_each_threads_stack() {
        let var = 0x5000;
        let mut p = Profiler::new(0x100, Some(var));
        p.record_word_store(var, 1);
        run(&mut p, 0x300, 1);
        p.record_call(0x310, 0, 0x304, SP);
        run(&mut p, 0x310, 2);

        p.record_word_store(var, 2);
        run(&mut p, 0x400, 5);

        // Back on thread 1, its return still finds its frame.
        p.record_word_store(var, 1);
        p.record_jump(0x304, SP);
        let f = stats(&p, 0x310);
        assert_eq!((f.self_count, f.inclusive_count), (2, 2));
    }

    #[test]
    fn a_store_elsewhere_does_not_switch_threads() {
        let mut p = Profiler::new(0x100, Some(0x5000));
        p.record_word_store(0x5004, 7);
        assert_eq!(p.current_thread, BOOT_THREAD);
    }

    #[test]
    fn an_iret_unwinds_frames_a_switch_left_above_the_interrupt() {
        let mut p = Profiler::new(0x100, None);
        p.record_interrupt(0x900, 0x104, SP);
        p.record_call(0x920, 0, 0x908, SP - 32);
        p.record_call(0x940, 0, 0x928, SP - 48);
        p.record_jump(0x104, SP);
        assert_eq!(p.current.frames.len(), 1);
    }

    #[test]
    fn the_same_pc_in_two_spaces_is_two_entries() {
        let mut p = Profiler::new(0x100, None);
        assert!(p.record_instruction(0x20000, 0x7000, 0));
        assert!(p.record_instruction(0x20000, 0x8000, 0));
        assert!(!p.record_instruction(0x20000, 0x8000, 0));
        assert_eq!(p.pc_hits[&(0x7000, 0x20000)], 1);
        assert_eq!(p.pc_hits[&(0x8000, 0x20000)], 2);
    }

    #[test]
    fn a_page_directory_holding_another_program_is_a_new_space() {
        let mut p = Profiler::new(0x100, None);
        p.enter_space(0x7000, None);
        assert_eq!(p.user_space(), 0x7000);
        p.record_instruction(0x20000, 0x7000, 0);
        p.set_text_sample(0x7000, 0x20000, vec![1, 2]);
        assert_eq!(p.text_base(0x7000), Some(0x20000));

        p.enter_space(0x7000, Some(&[1, 2]));
        assert_eq!(p.user_space(), 0x7000);
        p.enter_space(0x7000, Some(&[3, 4]));
        assert_eq!(p.user_space(), 0x7001);
    }

    #[test]
    fn reset_clears_counts_and_keeps_the_stack() {
        let mut p = Profiler::new(0x100, None);
        p.record_call(0x200, 0, 0x108, SP);
        run(&mut p, 0x200, 4);
        p.reset();
        run(&mut p, 0x210, 1);
        p.record_jump(0x108, SP);
        let f = stats(&p, 0x200);
        assert_eq!((f.self_count, f.inclusive_count), (1, 1));
        assert_eq!(p.total_instructions, 1);
    }
}
