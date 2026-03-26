use clap::Parser;
mod cli_runner;

fn main() {
    let cli = pm::cli::Cli::parse();
    if let Err(e) = cli_runner::run(cli) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
