use std::env;

#[derive(Default)]
pub struct Args {
    pub input_file: String,
    pub output_file: Option<String>,
    pub log_dir: Option<String>,
    // Data register index from --reg, validated while parsing.
    pub target_reg: Option<usize>,
    pub verbose: bool,
    pub trace: bool,
    pub break_addr: Option<u32>,
    pub step_count: Option<u64>,
    pub print_regs: bool,
    pub mem_range: Option<(u32, u32)>,
    pub headless: bool,
    // MYOS-004: run headless and drive the machine from JSON Lines commands on
    // stdin instead of executing to completion. See control_stdio.rs.
    pub control_stdio: bool,
    pub disk_file: Option<String>,
    // When set, collect an instruction-level profile and write it as JSON here.
    pub profile_out: Option<String>,
    // The kernel word holding the current thread, so the profile keeps one call
    // stack per thread. The name comes from the kernel's symbol map.
    pub profile_thread_var: Option<u32>,
    pub timer_interval: Option<u64>,
    // Report wall-clock time spent in each host display/input layer at exit.
    pub io_stats: bool,
    // Write the first 64 KB of RAM to memory_dump.txt when the run ends.
    pub memory_dump: bool,
    // Save the displayed frame here when the run ends (after --step, a halt,
    // or the window closing). PNG or PPM by extension.
    pub screenshot: Option<String>,
}

impl Args {
    /// True when the user asked to inspect the run; such runs print only what
    /// was requested, not the default end-of-run reports.
    pub fn debug_mode(&self) -> bool {
        self.trace
            || self.break_addr.is_some()
            || self.step_count.is_some()
            || self.print_regs
            || self.mem_range.is_some()
    }
}

/// Reads a decimal or 0x-prefixed hex number for `option_name`. Anything that
/// is not a number or does not fit in `T` (u32 or u64) is an error.
fn parse_number<T: TryFrom<u64>>(raw: &str, option_name: &str) -> Result<T, String> {
    let parsed = match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => raw.parse::<u64>().ok(),
    };
    let bits = 8 * std::mem::size_of::<T>();
    parsed
        .and_then(|value| T::try_from(value).ok())
        .ok_or_else(|| format!("{option_name} expects a valid u{bits} value, got '{raw}'"))
}

fn parse_register_name(raw: &str) -> Result<usize, String> {
    match raw.as_bytes() {
        [b'R', digit @ b'0'..=b'7'] => Ok((digit - b'0') as usize),
        _ => Err(format!("Invalid register name: {}", raw)),
    }
}

/// The value that must follow `flag`; without one, says the flag requires `hint`.
fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
    hint: &str,
) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} requires {hint}"))
}

pub fn parse_args() -> Result<Args, String> {
    parse_arg_list(env::args().skip(1))
}

fn parse_arg_list(mut argv: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = Args::default();

    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-i" | "--in" => {
                args.input_file = next_value(&mut argv, "-i/--in", "a filename argument")?;
            }
            "-o" | "--out" => {
                args.output_file = Some(next_value(&mut argv, "-o/--out", "a filename argument")?);
            }
            "--log-dir" => {
                args.log_dir = Some(next_value(&mut argv, "--log-dir", "a directory argument")?);
            }
            "--reg" => {
                let raw = next_value(&mut argv, "--reg", "a register name like R0..R7")?;
                args.target_reg = Some(parse_register_name(&raw)?);
            }
            "-v" | "--verbose" => args.verbose = true,
            "--trace" => args.trace = true,
            "--break" => {
                let raw = next_value(&mut argv, "--break", "an address like 0x40")?;
                args.break_addr = Some(parse_number(&raw, "--break")?);
            }
            "--step" => {
                let raw = next_value(&mut argv, "--step", "a count like 10")?;
                args.step_count = Some(parse_number(&raw, "--step")?);
            }
            "--regs" => args.print_regs = true,
            "--screenshot" => {
                args.screenshot = Some(next_value(
                    &mut argv,
                    "--screenshot",
                    "a file path (.png or .ppm)",
                )?);
            }
            "--mem" => {
                let start_raw = next_value(&mut argv, "--mem", "<addr> <len>")?;
                let len_raw = next_value(&mut argv, "--mem", "<addr> <len>")?;
                args.mem_range = Some((
                    parse_number(&start_raw, "--mem")?,
                    parse_number(&len_raw, "--mem")?,
                ));
            }
            "--headless" => args.headless = true,
            "--control-stdio" => {
                // Implies --headless: there is no window to drive by hand once
                // a client is talking to the machine over stdio.
                args.headless = true;
                args.control_stdio = true;
            }
            "--memory-dump" => args.memory_dump = true,
            "--io-stats" => args.io_stats = true,
            "--disk" => {
                args.disk_file = Some(next_value(&mut argv, "--disk", "a disk image path")?);
            }
            "--profile-thread-var" => {
                let raw = next_value(
                    &mut argv,
                    "--profile-thread-var",
                    "an address like 0x1D7E14",
                )?;
                args.profile_thread_var = Some(parse_number(&raw, "--profile-thread-var")?);
            }
            "--profile" => {
                args.profile_out = Some(next_value(&mut argv, "--profile", "an output JSON path")?);
            }
            "--timer-interval" => {
                let raw = next_value(&mut argv, "--timer-interval", "a microsecond count")?;
                args.timer_interval = Some(parse_number(&raw, "--timer-interval")?);
            }
            _ => {
                return Err(format!("Unknown option: {}", arg));
            }
        }
    }

    if args.input_file.is_empty() {
        return Err("Usage: myemulator -i <binary_file> [-o <output_file>]".to_string());
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::{parse_arg_list, parse_register_name, Args};

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_arg_list(args.iter().map(|arg| arg.to_string()))
    }

    fn rejection(args: &[&str]) -> String {
        parse(args).err().expect("the arguments should be rejected")
    }

    #[test]
    fn register_names_cover_r0_to_r7_only() {
        assert_eq!(parse_register_name("R0"), Ok(0));
        assert_eq!(parse_register_name("R7"), Ok(7));
        for invalid in ["", "R", "R8", "R10", "r1", "SP"] {
            assert_eq!(
                parse_register_name(invalid),
                Err(format!("Invalid register name: {}", invalid))
            );
        }
        assert_eq!(rejection(&["--reg", "R8"]), "Invalid register name: R8");
    }

    #[test]
    fn an_option_missing_its_value_says_what_it_needs() {
        for (flag, message) in [
            ("-i", "-i/--in requires a filename argument"),
            ("--in", "-i/--in requires a filename argument"),
            ("-o", "-o/--out requires a filename argument"),
            ("--out", "-o/--out requires a filename argument"),
            ("--log-dir", "--log-dir requires a directory argument"),
            ("--reg", "--reg requires a register name like R0..R7"),
            ("--break", "--break requires an address like 0x40"),
            ("--step", "--step requires a count like 10"),
            (
                "--screenshot",
                "--screenshot requires a file path (.png or .ppm)",
            ),
            ("--mem", "--mem requires <addr> <len>"),
            ("--disk", "--disk requires a disk image path"),
            (
                "--profile-thread-var",
                "--profile-thread-var requires an address like 0x1D7E14",
            ),
            ("--profile", "--profile requires an output JSON path"),
            (
                "--timer-interval",
                "--timer-interval requires a microsecond count",
            ),
        ] {
            assert_eq!(rejection(&["-i", "a.mbin", flag]), message);
        }
        assert_eq!(rejection(&["--mem", "zz"]), "--mem requires <addr> <len>");
    }

    #[test]
    fn numbers_are_decimal_or_hex_and_must_fit_their_option() {
        let args = parse(&[
            "-i",
            "a.mbin",
            "--break",
            "0x40",
            "--step",
            "18446744073709551615",
            "--mem",
            "0X10",
            "+8",
            "--profile-thread-var",
            "4294967295",
            "--timer-interval",
            "0x3e8",
        ])
        .unwrap();
        assert_eq!(args.break_addr, Some(0x40));
        assert_eq!(args.step_count, Some(u64::MAX));
        assert_eq!(args.mem_range, Some((0x10, 8)));
        assert_eq!(args.profile_thread_var, Some(u32::MAX));
        assert_eq!(args.timer_interval, Some(1000));

        for (args, message) in [
            (
                &["--break", "zz"][..],
                "--break expects a valid u32 value, got 'zz'",
            ),
            (
                &["--break", "0x100000000"],
                "--break expects a valid u32 value, got '0x100000000'",
            ),
            (
                &["--break", "0x"],
                "--break expects a valid u32 value, got '0x'",
            ),
            (
                &["--step", "-1"],
                "--step expects a valid u64 value, got '-1'",
            ),
            (
                &["--step", "18446744073709551616"],
                "--step expects a valid u64 value, got '18446744073709551616'",
            ),
            (
                &["--mem", "zz", "1"],
                "--mem expects a valid u32 value, got 'zz'",
            ),
            (
                &["--mem", "0", "4294967296"],
                "--mem expects a valid u32 value, got '4294967296'",
            ),
            (
                &["--profile-thread-var", "x"],
                "--profile-thread-var expects a valid u32 value, got 'x'",
            ),
            (
                &["--timer-interval", "1e3"],
                "--timer-interval expects a valid u64 value, got '1e3'",
            ),
        ] {
            assert_eq!(rejection(args), message);
        }
    }

    #[test]
    fn unknown_options_and_a_missing_input_are_rejected() {
        let usage = "Usage: myemulator -i <binary_file> [-o <output_file>]";
        assert_eq!(rejection(&[]), usage);
        assert_eq!(rejection(&["-i", ""]), usage);
        assert_eq!(rejection(&["-o", "out.txt"]), usage);
        assert_eq!(rejection(&["--bogus", "-i"]), "Unknown option: --bogus");
    }

    #[test]
    fn flags_set_their_fields_and_control_stdio_implies_headless() {
        let args = parse(&["--in", "a.mbin", "--control-stdio", "--regs", "-v"]).unwrap();
        assert_eq!(args.input_file, "a.mbin");
        assert!(args.control_stdio && args.headless && args.print_regs && args.verbose);
        assert!(args.output_file.is_none() && args.step_count.is_none());
    }
}
