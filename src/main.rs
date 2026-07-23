mod app;
mod cli;
mod constants;
mod instruction;
mod loader;
mod machine;
mod signals;

fn main() {
    signals::install();
    if let Err(err) = app::run() {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}
