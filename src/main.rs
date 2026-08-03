//! CLI entrypoint: parses arguments and either prints help/version or starts the JACK host.
//!
//! Entirely outside the real-time audio callback, so the callback contract's
//! no-I/O rule (docs/contracts.md §6) doesn't apply to `eprintln!` here.
#![allow(clippy::disallowed_macros)]

use std::process::ExitCode;

use clap::Parser;
use oxtt::{cli::Cli, jack_host, params::OttParams};

#[cfg(feature = "pi-controls")]
use oxtt::control::{ControlHandle, DEFAULT_POLL_INTERVAL, PiControlError, PiControls};

/// Starts the Raspberry Pi control surface if `--controls` asked for it.
///
/// Kept separate from `main` so the failure path is one `?` rather than a
/// second nested `match`, and so the `#[cfg]` covers a single item.
#[cfg(feature = "pi-controls")]
fn spawn_control_surface(
    enabled: bool,
    params: OttParams,
) -> Result<Option<ControlHandle>, PiControlError> {
    if !enabled {
        return Ok(None);
    }

    // `params` seeds both the mapping and the triple buffer, so a callback
    // that runs before the first poll sees the CLI values rather than a
    // default; the four pot-driven fields are the hardware's from the first
    // successful read onward.
    let source = PiControls::new()?;
    Ok(Some(ControlHandle::spawn(
        source,
        params,
        DEFAULT_POLL_INTERVAL,
    )))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let report_xruns_on_exit = cli.report_xruns_on_exit;
    #[cfg(feature = "pi-controls")]
    let controls_requested = cli.controls;
    let params = match OttParams::try_from(cli) {
        Ok(params) => params,
        Err(e) => {
            eprintln!("oxtt: {e}");
            return ExitCode::FAILURE;
        }
    };

    // A control surface that cannot be acquired is fatal here, reported the
    // same way as invalid parameters: the user asked for hardware that is not
    // answering, and starting anyway would silently give them a JACK client
    // whose knobs do nothing. A read failure once running is the opposite
    // case and is survivable — see `ControlHandle::spawn`.
    #[cfg(feature = "pi-controls")]
    let control = match spawn_control_surface(controls_requested, params) {
        Ok(control) => control,
        Err(e) => {
            eprintln!("oxtt: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Without the `pi-controls` feature there is no hardware source to build,
    // so every host runs exactly as it did before the control surface existed.
    #[cfg(not(feature = "pi-controls"))]
    let control = None;

    match jack_host::run(params, control) {
        Ok(summary) => {
            if report_xruns_on_exit {
                // This line is matched whole by the Raspberry Pi verification
                // scripts under `scripts/`, so it keeps its exact wording and
                // stays alone on its line.
                eprintln!("oxtt: xrun_count={}", summary.xrun_count());
                // The control-read count rides along on the same flag rather
                // than getting one of its own — both are running totals only
                // the host can print, because the threads that accumulate them
                // must not (docs/contracts.md §6, and the control thread's own
                // throttled stderr) — but only when there was a control
                // surface to count for.
                if let Some(failures) = summary.control_read_failures() {
                    eprintln!("oxtt: control_read_failures={failures}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("oxtt: {e}");
            ExitCode::FAILURE
        }
    }
}
