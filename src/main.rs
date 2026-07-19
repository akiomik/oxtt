//! CLI entrypoint: parses arguments and either prints help/version or starts the JACK host.

use std::env;
use std::process::ExitCode;

use oxtt::{jack_host, params};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    match params::parse_args(&args) {
        Ok(params::CliOutcome::Help(text) | params::CliOutcome::Version(text)) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Ok(params::CliOutcome::Run(ott_params)) => match jack_host::run(ott_params) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("oxtt: {e}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("oxtt: {e}");
            ExitCode::FAILURE
        }
    }
}
