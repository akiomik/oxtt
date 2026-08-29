//! CLI entrypoint for hyperglare's Bela host: parses arguments and starts the
//! audio system (docs/cross-compile.md, ADR 0011).
//!
//! Entirely outside the real-time audio callbacks, so the callback contract's
//! no-I/O rule (`docs/effectkit/realtime.md`) does not apply to `eprintln!`
//! here.
#![allow(clippy::disallowed_macros)]
// Off-device only the fallback `main` below can run, so the argument parsing
// and conversion above it are dead there — but they must still compile and
// lint, because that is where they are developed and tested.
#![cfg_attr(
    not(bela_device),
    allow(
        dead_code,
        reason = "only the fallback main is reachable off-device; the rest must still compile and lint"
    )
)]

use std::process::ExitCode;

use clap::Parser;
use hyperglare_args::chord_hz;
#[cfg(bela_device)]
use hyperglare_bela::run;
use hyperglare_bela::{BelaCli, RunOptions};
use hyperglare_dsp::processor::HyperglareParams;

/// Parses the command line into the three things a run needs.
///
/// Split out so that both `main`s agree on what a command line means, and so
/// that the error goes to the same place on either.
fn configure() -> (HyperglareParams, Vec<f32>, RunOptions) {
    let cli = BelaCli::parse();
    let options = RunOptions::from(&cli);
    let params = HyperglareParams::from(&cli.params);
    (params, chord_hz(&cli.notes), options)
}

#[cfg(bela_device)]
fn main() -> ExitCode {
    let (params, notes_hz, options) = configure();

    // The run's diagnostics are printed by the application's `cleanup`, not
    // here: `Bela::until_stopped` consumes the audio system without handing
    // the application back, so the counters do not outlive it.
    match run(params, notes_hz, &options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hyperglare: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Off-device this binary can be built and its arguments checked, but there is
/// no audio system to start — `bela`'s device code is behind a `bela_device`
/// cfg its build script only sets for aarch64 Linux.
///
/// It still parses the command line first, so that `--help`, `--version` and
/// a bad argument behave the same way they will on the board.
#[cfg(not(bela_device))]
fn main() -> ExitCode {
    let _ = configure();
    eprintln!(
        "hyperglare: this binary must be cross-compiled for Bela Gem (aarch64-unknown-linux-gnu); \
         see docs/cross-compile.md"
    );
    ExitCode::FAILURE
}
