# Development

## Prerequisites

- Rust, edition 2024 (rustc >= 1.85).
- A JACK server, or a JACK-compatible backend (e.g. PipeWire's JACK compatibility layer), to run the `oxtt` binary. Not required to build the crate or run `cargo test`.

## Build

```sh
cargo build
cargo build --release
```

The repository's `rust-toolchain.toml` pins the development toolchain and is
selected automatically by `rustup` while the current directory is inside the
repository. `Cargo.toml`'s `rust-version` has a different purpose: it declares
the minimum supported Rust version used by the MSRV CI job. Do not replace the
pinned development toolchain with the MSRV just to build a release binary.
The toolchain uses the `minimal` profile to avoid downloading local Rust
documentation on headless systems; `clippy` and `rustfmt` remain explicitly
listed as required components.

For a reproducible binary, including the Raspberry Pi build, use the lockfile:

```sh
cargo build --release --locked
```

## Raspberry Pi 5 Native Build

The Raspberry Pi 5 baseline is a native build on 64-bit Raspberry Pi OS Lite.
This is both the host and target platform `aarch64-unknown-linux-gnu`, so no
additional target is needed in `rust-toolchain.toml` and no `--target` argument
is needed.

Install the native build and JACK dependencies on the Pi:

```sh
sudo apt update
sudo apt install build-essential pkg-config git curl file jackd2 libjack-jackd2-dev
```

Install `rustup` without an unrelated default toolchain. The first Rust command
run in the repository will install the version and components declared by
`rust-toolchain.toml`:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --profile minimal --default-toolchain none
source "${HOME}/.cargo/env"
git clone https://github.com/akiomik/oxtt.git
cd oxtt
rustup show active-toolchain
rustc -vV
cargo build --release --locked
file target/release/oxtt
ldd target/release/oxtt
```

Before running the binary, confirm that `rustc -vV` reports
`host: aarch64-unknown-linux-gnu`, `file` reports an AArch64 ELF binary, and
`ldd` resolves `libjack.so.0`.

For the full reproducible setup on real hardware — the build/run split, realtime
privileges, ALSA card naming, JACK port mapping, and the audio-stability and
latency verification with a class-compliant USB audio interface — see
[`raspberry-pi/usb-audio-setup.md`](raspberry-pi/usb-audio-setup.md) and
[`raspberry-pi/usb-audio-verification.md`](raspberry-pi/usb-audio-verification.md).

### The `pi-controls` feature

The physical control surface (`src/control/pi.rs`: MCP3008 pots over SPI0/CE0, a
bypass switch on GPIO17) is behind the optional `pi-controls` Cargo feature,
which is **off by default**. `rppal` is Linux-only, so a default build stays
buildable on macOS and every host runs exactly as it did before the control
surface existed. Enable it on the Pi:

```sh
cargo build --release --locked --features pi-controls
```

The feature only compiles the hardware layer in; it does not turn it on. The
`--controls` flag — which exists only in a `pi-controls` build — is what starts
the control thread, so the same binary still runs on a Pi with no breadboard
attached. See [`raspberry-pi/control-surface-setup.md`](raspberry-pi/control-surface-setup.md)
for wiring the hardware and enabling SPI0,
[`raspberry-pi/control-surface-verification.md`](raspberry-pi/control-surface-verification.md)
for the hardware verification, and
[`decisions/0010-three-layer-control-surface-and-newest-value-handoff.md`](decisions/0010-three-layer-control-surface-and-newest-value-handoff.md)
for the design.

Because `rppal` cannot compile on macOS, two kinds of command **fail there**: any
workspace-wide one (`cargo build --workspace`, `cargo clippy --workspace
--all-targets`), because the `oxtt-pi-tools` crate depends on `rppal`
unconditionally; and any command that enables `pi-controls`. Scope macOS work to
`-p oxtt` (or plain `cargo build`/`cargo clippy --all-targets`, which already
build only the root package) and leave the feature off.

The feature-gated module can still be type-checked from macOS by
cross-compiling. Nothing links, so no Linux linker or sysroot is needed:

```sh
rustup target add aarch64-unknown-linux-gnu
PKG_CONFIG_ALLOW_CROSS=1 cargo clippy -p oxtt --features pi-controls --all-targets --target aarch64-unknown-linux-gnu -- -D warnings
```

`PKG_CONFIG_ALLOW_CROSS=1` is required because `jack-sys`'s build script
otherwise refuses to run `pkg-config` for a foreign target. Since `cargo
check`/`cargo clippy` never link, that is sufficient to type-check and lint
`src/control/pi.rs` without a Pi in reach — it is not a way to produce a
runnable binary (see the next section).

CI covers the feature natively on Linux in the `pi-controls` job, which lints,
tests, and builds it on an `ubuntu-latest` runner.

### Why macOS cross-compilation is not the baseline

Adding `aarch64-unknown-linux-gnu` to `rust-toolchain.toml` only installs that
target's Rust standard library. A macOS host would still need a Linux/AArch64
linker and sysroot, plus target-architecture JACK libraries and `pkg-config`
configuration for `jack-sys`. That setup is more moving parts than a native Pi
build and would make every developer install a target that most builds do not
use.

For Raspberry Pi verification, transfer the source with Git and build on the
Pi. Introduce a containerized cross-build workflow (and then add the Rust target
explicitly) only if native build time or repeated deployment becomes a measured
problem.

## Format and Lint

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Tests

```sh
cargo test --all-targets
```

Separately, `cargo test --release` also proves `OttProcessor::process`/`reset` and `ControlMapping::update` panic-free (docs/contracts.md §6); the proof only holds under full optimization, so it doesn't run as part of the plain debug-mode suite above.

The suite is organized by module and none of it requires a running JACK server:

- `src/cli.rs` — CLI argument parsing
- `src/params/` — parameter value objects, validation, and presets
- `src/dsp.rs` — `OttProcessor` unit tests and processor-level integration tests
- `src/dsp/crossover.rs` — crossover reconstruction and phase-compensator tests
- `src/dsp/compressor.rs` — dual-threshold gain computation tests
- `src/dsp/envelope.rs` — envelope follower and time-scaling tests
- `src/dsp/smooth.rs` — parameter-smoothing tests
- `src/control/` — control-surface conditioning (jitter filter, deadband, bypass latch), the control thread and its handoff, and, only under `--features pi-controls`, the MCP3008 command/response encoding

See `contracts.md` for the guarantees those tests protect.

## Inspecting Generated Code

`cargo-show-asm` shows the assembly rustc actually generates for a function, which is the only way to confirm whether a hot DSP function was inlined rather than guessing from `#[inline]` annotations alone:

```sh
cargo install cargo-show-asm
cargo asm -p oxtt --lib "OttProcessor::process"
```

Narrowing to an inner function name (e.g. `db_to_amp`, `process_frame`, `envelope_coefficient`, `update_envelope`) currently reports no match: `[profile.release] codegen-units = 1` together with their `#[inline]` annotations already fully inlines them into `OttProcessor::process`, which is the only real-time-path function with a standalone symbol. Re-check with this tool before adding `#[inline(always)]` anywhere; the compiler may already be doing the work.

## Running Locally Without Real Audio Hardware

`oxtt` connects to whichever JACK server is already running, under the client name `oxtt`, and registers four ports (`input_l`, `input_r`, `output_l`, `output_r`) without auto-connecting them. To develop without an audio interface:

```sh
jackd -d dummy &
cargo run --release -- --preset safe-start
```

Connect ports with whichever tool is available in your environment:

- `jack_connect` / `jack_lsp`, if your JACK install ships example clients (Homebrew's `jack2` bottle does not, by default).
- QjackCtl or another GUI patchbay.
- The bundled helper binaries in the `oxtt-jack-tools` crate, which depend only on the `jack` crate and work in every environment:

  ```sh
  cargo run -p oxtt-jack-tools --bin list_ports
  cargo run -p oxtt-jack-tools --bin connect_ports -- oxtt:input_l system:capture_1
  ```

## Manual Smoke Test

1. Start `jackd` (a real backend, or `-d dummy`).
2. `cargo run --release -- --preset safe-start`.
3. Confirm 4 ports are registered (`list_ports` helper, or `jack_lsp`).
4. Connect ports (`connect_ports` helper, `jack_connect`, or a GUI patchbay) and confirm continuous processing at any buffer size.
5. Send SIGINT (Ctrl-C) and confirm clean shutdown.
6. Stop the JACK server while `oxtt` is running and confirm it exits instead of hanging.

## macOS Notes

### Running the binary: `DYLD_LIBRARY_PATH`

On macOS, the `jack` crate links against `libjack` dynamically, and the Homebrew-installed library is not always found on the default dynamic linker search path. If `cargo run`/the built binary fails to find `libjack` at startup, set:

```sh
DYLD_LIBRARY_PATH=/opt/homebrew/lib cargo run --release -- --preset safe-start
```

(adjust the path if Homebrew is installed under `/usr/local` instead of `/opt/homebrew`, e.g. on an Intel Mac).

### Verifying against real audio with QjackCtl

Homebrew's `jack2` bottle does not include the `jack_lsp`/`jack_connect` CLI tools (see `cargo run -p oxtt-jack-tools --bin list_ports`/`connect_ports` above for a CLI-tool-free alternative). For interactive verification, QjackCtl is the easiest option:

```sh
brew install --cask qjackctl
```

QjackCtl needs its own Qt plugin path on Homebrew's build; without it, the app may fail to start:

```sh
QT_PLUGIN_PATH=/opt/homebrew/opt/qtbase/share/qt/plugins qjackctl
```

By default, QjackCtl connects to the `jackd` instance already started by Homebrew (`brew services start jack`), rather than starting its own. With `oxtt` running and connected to that same `jackd`:

1. Open QjackCtl's Graph view.
2. Connect `system:capture_1` -> `oxtt:input_l`/`input_r`.
3. Connect `oxtt:output_l`/`output_r` -> `system:playback_1`/`playback_2`.

Once wired this way, `oxtt` is active in the live signal path.

To verify against a real audio interface instead of the default device, `jackd` must be stopped first so QjackCtl can start its own instance with the interface selected:

```sh
brew services stop jack
```

Then, in QjackCtl's Setup dialog, select the audio interface and sample rate, and press Start.
