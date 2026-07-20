//! Development helper that connects to JACK and lists registered ports (docs/development.md).
//!
//! Lets JACK smoke tests be automated even in development environments where
//! CLI tools like `jack_lsp` aren't available. Depends only on the `jack` crate.
//!
//! Not on the real-time audio callback path, so the callback contract's
//! no-I/O rule (docs/contracts.md §6) doesn't apply to `println!`/`eprintln!` here.
#![allow(clippy::disallowed_macros)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let (client, _status) =
        match jack::Client::new("oxtt_list_ports", jack::ClientOptions::default()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("list_ports: failed to connect to JACK: {e}");
                return ExitCode::FAILURE;
            }
        };

    for port_name in client.ports(None, None, jack::PortFlags::empty()) {
        println!("{port_name}");
    }

    ExitCode::SUCCESS
}
