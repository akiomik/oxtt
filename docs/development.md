# Development

## Prerequisites

- Rust, edition 2024 (rustc >= 1.88).
- A JACK server, or a JACK-compatible backend (e.g. PipeWire's JACK compatibility layer), to run the `oxtt` binary. Not required to build the crate or run `cargo test`.
- For the Bela host only: a cross toolchain and a sysroot from the board — see [`bela/cross-compile.md`](bela/cross-compile.md). Not required to build, lint or test the Bela host's portable half, which is almost all of it.

## Hosts and features

There are two host adapters, one per feature, and they are never both in one binary:

| Feature | Binary | What it needs |
| --- | --- | --- |
| `jack-host` (default) | `oxtt` | a JACK server to run; `libjack` headers to build |
| `bela-host` | `oxtt-bela` | a Bela Gem Stereo to run; nothing extra to build or test off-device |
| `pi-controls` | — | a Raspberry Pi; `rppal`, which is Linux-only |

`pi-controls` is a control surface for the JACK host, not a host of its own.

Everything below the adapters — the DSP, the parameters, the presets, the control surface's mapping layer — builds with **neither** host feature enabled, and CI asserts that:

```sh
cargo clippy -p oxtt --no-default-features --all-targets -- -D warnings
```

This matters more than it looks: it is what kept adding a second host to writing an adapter rather than unpicking the first one.

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
scripts/pi-build.sh
file target/release/oxtt
ldd target/release/oxtt
```

`scripts/pi-build.sh` is `cargo build --release --locked` with
`-C target-cpu=cortex-a76` exported for it. That flag used to live in
`.cargo/config.toml` under `[target.aarch64-unknown-linux-gnu]`, and cannot any
more: a Bela Gem compiles for the same triple and is a Cortex-A53, so one
section describing the Pi would silently mis-build the Bela and vice versa.
Each build now exports its own settings from its own script — see
[`bela/cross-compile.md`](bela/cross-compile.md) for the other half.

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

## Bela Host

The Bela host is the opposite case: it needs no extra anything to work on. The
`bela` crate puts its device code behind a `bela_device` cfg its build script
sets only for aarch64 Linux, so on macOS — and on an ordinary CI runner — the
application type, the control conversion and their tests are ordinary code:

```sh
cargo clippy -p oxtt --no-default-features --features bela-host --all-targets -- -D warnings
cargo test  -p oxtt --no-default-features --features bela-host --all-targets
```

The half behind that cfg is compiled by cross-*checking*, which links nothing
and so needs neither `libbela` nor a sysroot:

```sh
rustup target add aarch64-unknown-linux-gnu
cargo clippy -p oxtt --no-default-features --features bela-host --all-targets \
  --target aarch64-unknown-linux-gnu -- -D warnings
```

Producing a runnable binary *is* a real cross-compile, and that is
[`bela/cross-compile.md`](bela/cross-compile.md).

CI runs all three: `bela-host` for the portable half, `bela-device` for the
cfg'd half, and `no-host` for the shared code with neither host enabled.

### Why macOS cross-compilation is not the baseline for the Pi

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

This is a judgement about the Pi, not about cross-compilation. The Bela host
*is* cross-compiled, because there is no alternative: the board is not a build
host. What makes it worth the moving parts there is that the parts are fewer —
`bela-sys` derives the linker arguments from the sysroot and relays them
through Cargo metadata, so there is no `pkg-config` question and nothing to
copy into this repository.

## Format and Lint

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Tests

```sh
cargo test --all-targets
```

Separately, `cargo test --release` also proves `OttProcessor::process`/`process_frame`/`reset` and `ControlMapping::update` panic-free ([contracts.md §6](contracts.md#6-real-time-callback)); the proof only holds under full optimization, so it doesn't run as part of the plain debug-mode suite above.

The suite is organized by module and none of it requires a running JACK server:

- `src/cli.rs` — CLI argument parsing
- `src/params/` — parameter value objects, validation, and presets
- `src/dsp.rs` — `OttProcessor` unit tests and processor-level integration tests
- `src/dsp/crossover.rs` — crossover reconstruction and phase-compensator tests
- `src/dsp/compressor.rs` — dual-threshold gain computation tests
- `src/dsp/envelope.rs` — envelope follower and time-scaling tests
- `src/dsp/smooth.rs` — parameter-smoothing tests
- `src/control/` — control-surface conditioning (jitter filter, deadband, explicit bypass level), and, only under `--features jack-host`, the control thread and its handoff; only under `--features pi-controls`, the MCP3008 command/response encoding
- `src/bela_host/` — only under `--features bela-host`: the analog-reading-to-pot-position conversion and its boundaries, the read decimator, the settings the board is asked for, and the exit report's wording

See [contracts.md](contracts.md) for the guarantees those tests protect.

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
