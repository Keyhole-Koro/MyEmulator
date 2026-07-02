use std::env;

pub struct Args {
    pub input_file: String,
    pub output_file: String,
    pub log_dir: String,
    pub target_reg: String,
    pub verbose: bool,
    pub trace: bool,
    pub break_addr: Option<u32>,
    pub step_count: Option<u64>,
    pub print_regs: bool,
    pub mem_range: Option<(u32, u32)>,
    pub timer_interval: Option<u64>,
    pub headless: bool,
    pub disk_file: Option<String>,
}

fn parse_u32_value(raw: &str, option_name: &str) -> Result<u32, String> {
    if let Some(stripped) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        u32::from_str_radix(stripped, 16)
            .map_err(|_| format!("{} expects a valid u32 value, got '{}'", option_name, raw))
    } else {
        raw.parse::<u32>()
            .map_err(|_| format!("{} expects a valid u32 value, got '{}'", option_name, raw))
    }
}

fn parse_u64_value(raw: &str, option_name: &str) -> Result<u64, String> {
    if let Some(stripped) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        u64::from_str_radix(stripped, 16)
            .map_err(|_| format!("{} expects a valid u64 value, got '{}'", option_name, raw))
    } else {
        raw.parse::<u64>()
            .map_err(|_| format!("{} expects a valid u64 value, got '{}'", option_name, raw))
    }
}

pub fn parse_args() -> Result<Args, String> {
    let mut args = env::args().skip(1);

    let mut input_file = String::new();
    let mut output_file = String::new();
    let mut log_dir = String::new();
    let mut target_reg = String::new();
    let mut verbose = false;
    let mut trace = false;
    let mut break_addr = None;
    let mut step_count = None;
    let mut print_regs = false;
    let mut mem_range = None;
    let mut timer_interval = None;
    let mut headless = false;
    let mut disk_file = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-i" | "--in" => {
                input_file = args
                    .next()
                    .ok_or_else(|| "-i/--in requires a filename argument".to_string())?;
            }
            "-o" | "--out" => {
                output_file = args
                    .next()
                    .ok_or_else(|| "-o/--out requires a filename argument".to_string())?;
            }
            "--log-dir" => {
                log_dir = args
                    .next()
                    .ok_or_else(|| "--log-dir requires a directory argument".to_string())?;
            }
            "--reg" => {
                target_reg = args
                    .next()
                    .ok_or_else(|| "--reg requires a register name like R0..R7".to_string())?;
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            "--trace" => {
                trace = true;
            }
            "--break" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "--break requires an address like 0x40".to_string())?;
                break_addr = Some(parse_u32_value(&raw, "--break")?);
            }
            "--step" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "--step requires a count like 10".to_string())?;
                step_count = Some(parse_u64_value(&raw, "--step")?);
            }
            "--regs" => {
                print_regs = true;
            }
            "--timer-interval" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "--timer-interval requires a count like 1000".to_string())?;
                timer_interval = Some(parse_u64_value(&raw, "--timer-interval")?);
            }
            "--mem" => {
                let start_raw = args
                    .next()
                    .ok_or_else(|| "--mem requires <addr> <len>".to_string())?;
                let len_raw = args
                    .next()
                    .ok_or_else(|| "--mem requires <addr> <len>".to_string())?;
                mem_range = Some((
                    parse_u32_value(&start_raw, "--mem")?,
                    parse_u32_value(&len_raw, "--mem")?,
                ));
            }
            "--headless" => {
                headless = true;
            }
            "--disk" => {
                disk_file = Some(
                    args.next()
                        .ok_or_else(|| "--disk requires a disk image path".to_string())?,
                );
            }
            _ => {
                return Err(format!("Unknown option: {}", arg));
            }
        }
    }

    if input_file.is_empty() {
        return Err("Usage: myemulator -i <binary_file> [-o <output_file>]".to_string());
    }

    Ok(Args {
        input_file,
        output_file,
        log_dir,
        target_reg,
        verbose,
        trace,
        break_addr,
        step_count,
        print_regs,
        mem_range,
        timer_interval,
        headless,
        disk_file,
    })
}
