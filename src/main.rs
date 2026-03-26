use clap::Parser;
mod cli_runner;

fn main() {
    if std::env::args().any(|a| a == "--mcp") {
        pm::mcp::run_mcp_server();
        return;
    }
    let cli = pm::cli::Cli::parse();
    if let Err(e) = cli_runner::run(cli) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
