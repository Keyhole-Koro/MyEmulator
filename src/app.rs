use crate::cli::parse_args;
use crate::loader::read_binary_file;
use crate::machine::Machine;

pub fn run() -> Result<(), String> {
    let args = parse_args()?;

    println!("Loading binary from {}", args.input_file);
    let binary = read_binary_file(&args.input_file)?;

    let mut machine = Machine::new(args.verbose);
    let start_address = 0x0000_0000;
    machine.load_program(&binary, start_address);
    machine.set_instruction_pointer(start_address);
    machine.execute()?;

    machine.display_stack();

    let mut report = String::new();
    for i in 0..=7 {
        let unsigned_value = machine.get_data_register(i)?;
        let signed_value = unsigned_value as i32;
        report.push_str(&format!(
            "R{}: Unsigned={}, Signed={}, Binary={:032b}\n",
            i, unsigned_value, signed_value, unsigned_value
        ));
    }
    report.push_str(&format!("SP: {:x}\n", machine.stack_pointer()));
    report.push_str(&format!("BP: {:x}\n", machine.base_pointer()));
    report.push_str(&format!("PC: {:x}\n", machine.program_counter()));
    report.push_str(&format!("SR: {:x}\n", machine.status_register()));
    report.push_str(&format!("LR: {:x}\n", machine.link_register()));
    report.push_str(&format!(
        "Flags: Carry={}, Zero={}, Sign={}, Overflow={}\n",
        machine.carry_flag(),
        machine.zero_flag(),
        machine.sign_flag(),
        machine.overflow_flag()
    ));

    let dump_file = "memory_dump.txt";
    machine.dump_memory_text(dump_file, 0x0000_0000, 0x0000_FFFF)?;
    println!("Memory dump written to {}", dump_file);

    if args.output_file.is_empty() {
        print!("{}", report);
    } else {
        std::fs::write(&args.output_file, report)
            .map_err(|e| format!("Failed to open output file: {} ({})", args.output_file, e))?;
        println!("Output written to {}", args.output_file);
    }

    if !args.target_reg.is_empty() {
        let bytes = args.target_reg.as_bytes();
        if bytes.len() == 2 && bytes[0] == b'R' && bytes[1].is_ascii_digit() {
            let reg_num = (bytes[1] - b'0') as usize;
            if reg_num <= 7 {
                println!("{}", machine.get_data_register(reg_num)?);
                return Ok(());
            }
        }
        return Err(format!("Invalid register name: {}", args.target_reg));
    }

    Ok(())
}
