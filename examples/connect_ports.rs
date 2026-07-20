//! Development helper that connects two ports given their names (docs/development.md).
//!
//! Lets JACK smoke tests be automated even in development environments where
//! CLI tools like `jack_connect` aren't available. Depends only on the `jack` crate.
//!
//! Not on the real-time audio callback path, so the callback contract's
//! no-I/O rule (docs/contracts.md §6) doesn't apply to `eprintln!` here.
#![allow(clippy::disallowed_macros)]

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (source, destination) = if let (Some(s), Some(d)) = (args.first(), args.get(1)) {
        (s.as_str(), d.as_str())
    } else {
        eprintln!("usage: connect_ports <source_port> <destination_port>");
        return ExitCode::FAILURE;
    };

    let (client, _status) =
        match jack::Client::new("oxtt_connect_ports", jack::ClientOptions::default()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("connect_ports: failed to connect to JACK: {e}");
                return ExitCode::FAILURE;
            }
        };

    match client.connect_ports_by_name(source, destination) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("connect_ports: failed to connect {source} -> {destination}: {e}");
            ExitCode::FAILURE
        }
    }
}
