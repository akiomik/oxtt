//! CLI entrypoint: parses arguments and either prints help/version or starts the JACK host.

use std::process::ExitCode;

use clap::Parser;
use oxtt::{cli::Cli, jack_host, params::OttParams};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let params: OttParams = cli.into();
    match jack_host::run(params) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("oxtt: {e}");
            ExitCode::FAILURE
        }
    }
}
