//! CLI entrypoint: parses arguments and either prints help/version or starts the JACK host.
//!
//! Entirely outside the real-time audio callback, so the callback contract's
//! no-I/O rule (docs/contracts.md §6) doesn't apply to `eprintln!` here.
#![allow(clippy::disallowed_macros)]

use std::process::ExitCode;

use clap::Parser;
use oxtt::{cli::Cli, jack_host, params::OttParams};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let params = match OttParams::try_from(cli) {
        Ok(params) => params,
        Err(e) => {
            eprintln!("oxtt: {e}");
            return ExitCode::FAILURE;
        }
    };
    match jack_host::run(params) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("oxtt: {e}");
            ExitCode::FAILURE
        }
    }
}
