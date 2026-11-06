use crate::cli::{parse_args, Args};
use crate::control_stdio;
use crate::loader::read_binary_file;
use crate::machine::{DebugOptions, Machine};
use std::path::{Path, PathBuf};

const LOAD_ADDRESS: u32 = 0x0000_0000;
const MEMORY_DUMP_FILE: &str = "memory_dump.txt";
const MEMORY_DUMP_START: u32 = 0x0000_0000;
const MEMORY_DUMP_END: u32 = 0x0000_FFFF;

pub fn run() -> Result<(), String> {
    let args = parse_args()?;

    status(&args, &format!("Loading binary from {}", args.input_file));
    let binary = read_binary_file(&args.input_file)?;

    let mut machine = build_machine(&args)?;
    let log_dir = prepare_log_dir(&mut machine, &args)?;
    machine.load_program(&binary, LOAD_ADDRESS);
    machine.set_instruction_pointer(LOAD_ADDRESS);

    // Enable profiling before execution so the call graph is rooted at the entry
    // point and every retired instruction is counted.
    if args.profile_out.is_some() {
        machine.enable_profiler(LOAD_ADDRESS, args.profile_thread_var);
    }

    // MYOS-004: control-stdio mode drives the machine from JSON Lines commands
    // on stdin instead of running it to completion, so it exits here rather
    // than falling through to the end-of-run reporting below (that reporting
    // describes a halted machine, which this one never becomes).
    if args.control_stdio {
        control_stdio::run(&mut machine)?;
        if let Some(profile_path) = &args.profile_out {
            machine.write_profile(profile_path)?;
        }
        return Ok(());
    }

    execute(&mut machine, &args)?;
    write_run_outputs(&mut machine, &args)?;
    print_diagnostics(&machine, &args);
    write_final_state(&machine, &args, log_dir.as_deref())?;
    write_register_report(&machine, &args)?;
    print_target_register(&machine, &args)
}

/// Prints a startup message. In control-stdio mode stdout carries the JSON
/// Lines protocol, so human-readable text goes to stderr instead.
fn status(args: &Args, message: &str) {
    if args.control_stdio {
        eprintln!("{}", message);
    } else {
        println!("{}", message);
    }
}

fn build_machine(args: &Args) -> Result<Machine, String> {
    // Control-stdio drives the guest through the automation bridge and never
    // shows a window; forcing headless keeps X11 scanout and input polling
    // off the guest's critical path (a windowed control-stdio run mapped
    // VRAM ~10x slower, so os.ready timed out during boot -- MYOS-015).
    let mut machine = Machine::new(args.verbose, args.headless || args.control_stdio);
    if args.control_stdio {
        machine.enable_control_stdio();
    }
    if let Some(interval) = args.timer_interval {
        machine.set_timer_interval(interval);
    }
    if let Some(disk) = &args.disk_file {
        machine.load_disk(PathBuf::from(disk))?;
        status(args, &format!("Mounted disk image {}", disk));
    }
    Ok(machine)
}

fn prepare_log_dir(machine: &mut Machine, args: &Args) -> Result<Option<PathBuf>, String> {
    let Some(log_dir) = &args.log_dir else {
        return Ok(None);
    };
    let dir = PathBuf::from(log_dir);
    std::fs::create_dir_all(dir.join("memory"))
        .map_err(|e| format!("Failed to create log directory {}: {}", dir.display(), e))?;
    machine.set_serial_log(dir.join("serial.txt"))?;
    machine.set_trace_log(dir.join("trace.log"))?;
    Ok(Some(dir))
}

fn execute(machine: &mut Machine, args: &Args) -> Result<(), String> {
    if args.io_stats {
        machine.enable_io_stats();
    }
    machine.execute_with_debug(DebugOptions {
        trace: args.trace,
        break_addr: args.break_addr,
        step_count: args.step_count,
    })
}

/// Writes the outputs requested for this run: the profile and the screenshot.
fn write_run_outputs(machine: &mut Machine, args: &Args) -> Result<(), String> {
    if let Some(profile_path) = &args.profile_out {
        machine.write_profile(profile_path)?;
        println!("Profile written to {}", profile_path);
    }
    if let Some(shot) = &args.screenshot {
        machine.write_screenshot(shot)?;
        println!("Screenshot written to {}", shot);
    }
    Ok(())
}

fn print_diagnostics(machine: &Machine, args: &Args) {
    machine.report_io_stats();

    if !args.debug_mode() {
        machine.display_stack();
    }

    // The run loop already printed the registers where it paused.
    let already_printed_regs = args.step_count.is_some() || args.break_addr.is_some();
    if args.print_regs && !already_printed_regs {
        machine.print_registers();
    }

    if let Some((addr, len)) = args.mem_range {
        machine.dump_memory_range(addr, len);
    }
}

/// Saves the final memory image, plus the register state when logging.
fn write_final_state(machine: &Machine, args: &Args, log_dir: Option<&Path>) -> Result<(), String> {
    if !args.debug_mode() {
        machine.dump_memory_text(MEMORY_DUMP_FILE, MEMORY_DUMP_START, MEMORY_DUMP_END)?;
        println!("Memory dump written to {}", MEMORY_DUMP_FILE);
    }

    if let Some(dir) = log_dir {
        machine.dump_memory_text(
            dir.join("memory").join("final-00000000-0000ffff.txt"),
            MEMORY_DUMP_START,
            MEMORY_DUMP_END,
        )?;
        std::fs::write(dir.join("registers-hex.txt"), machine.register_report_hex())
            .map_err(|e| format!("Failed to write register log in {}: {}", dir.display(), e))?;
    }
    Ok(())
}

/// Sends the register report to `-o` when given. Without it the report goes to
/// stdout, except in debug runs, which print only what was asked for.
fn write_register_report(machine: &Machine, args: &Args) -> Result<(), String> {
    let report = machine.register_report();
    match &args.output_file {
        Some(path) => {
            std::fs::write(path, report)
                .map_err(|e| format!("Failed to open output file: {} ({})", path, e))?;
            println!("Output written to {}", path);
        }
        None if !args.debug_mode() => print!("{}", report),
        None => {}
    }
    Ok(())
}

/// Prints the `--reg` value last: test runners read it from the final line.
fn print_target_register(machine: &Machine, args: &Args) -> Result<(), String> {
    if let Some(register) = args.target_reg {
        println!("{}", machine.get_data_register(register)?);
    }
    Ok(())
}
