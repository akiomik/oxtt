//! Emits the `bela_device` cfg, and turns the device link arguments `bela`
//! relays into arguments for this crate's own binaries.
//!
//! `bela-sys` derives `--sysroot`, `-B` and `-Wl,-rpath-link` from
//! `BELA_SYSROOT` and publishes them as `links` metadata; `bela` republishes
//! them under `links = "bela_relay"`. Cargo hands `DEP_*` metadata only to an
//! *immediate* dependent's build script, so `oxtt-bela` cannot inherit them
//! and this script has to pass them on. Without it, linking a device binary
//! needs the linker wrapper script from the `bela-rs` repository instead
//! (docs/bela/cross-compile.md).
//!
//! `println!` is a build script's entire protocol with Cargo, and this runs at
//! build time on a development machine rather than on any audio path, so the
//! crate-wide prohibitions from `clippy.toml` (docs/contracts.md §6) do not
//! describe it.
#![allow(
    clippy::disallowed_macros,
    reason = "a build script talks to Cargo over stdout; docs/contracts.md §6 governs the audio callback, not build time"
)]

use std::env;

fn main() {
    emit_device_cfg();
    relay_link_args();
}

/// Mirrors `bela`'s own `bela_device` cfg onto this crate.
///
/// `bela/build.rs` sets it for `bela` alone — a `rustc-cfg` from a build
/// script reaches only the crate it belongs to — so the same condition has to
/// be repeated here for `#[cfg(bela_device)]` to mean anything in oxtt's own
/// source. The condition is the one `bela` uses, and must stay that way: it
/// decides whether `bela::Bela` exists to be named.
fn emit_device_cfg() {
    println!("cargo::rustc-check-cfg=cfg(bela_device)");
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if arch == "aarch64" && os == "linux" {
        println!("cargo::rustc-cfg=bela_device");
    }
}

/// Passes on the device link arguments `bela` relayed, if there are any.
///
/// Everything here is skipped unless the `bela-host` feature brought `bela`
/// into the build *and* the target is the device: off-device, `bela-sys`
/// publishes no metadata, so the count is simply absent. That is also why
/// nothing here is behind a `cfg(feature = ...)` — the environment already
/// says whether there is anything to do.
fn relay_link_args() {
    println!("cargo::rerun-if-env-changed=DEP_BELA_RELAY_LINK_ARGS_COUNT");

    let Ok(count) = env::var("DEP_BELA_RELAY_LINK_ARGS_COUNT") else {
        return;
    };
    let Ok(count) = count.parse::<usize>() else {
        println!("cargo::error=DEP_BELA_RELAY_LINK_ARGS_COUNT is not a number: {count}");
        return;
    };
    // Indexed rather than one whitespace-separated string, so that a
    // `BELA_SYSROOT` containing a space survives the trip.
    for index in 0..count {
        let key = format!("DEP_BELA_RELAY_LINK_ARGS_{index}");
        let Ok(arg) = env::var(&key) else {
            println!("cargo::error={key} is missing, but the count promised {count} arguments");
            return;
        };
        println!("cargo::rustc-link-arg={arg}");
    }
}
