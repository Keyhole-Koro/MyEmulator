use std::env;

pub struct Args {
    pub input_file: String,
    pub output_file: String,
    pub target_reg: String,
    pub verbose: bool,
}

pub fn parse_args() -> Result<Args, String> {
    let mut args = env::args().skip(1);

    let mut input_file = String::new();
    let mut output_file = String::new();
    let mut target_reg = String::new();
    let mut verbose = false;

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
            "--reg" => {
                target_reg = args
                    .next()
                    .ok_or_else(|| "--reg requires a register name like R0..R7".to_string())?;
            }
            "-v" | "--verbose" => {
                verbose = true;
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
        target_reg,
        verbose,
    })
}
