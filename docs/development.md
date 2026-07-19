# Development

## Prerequisites

- Rust, edition 2024 (rustc >= 1.85).
- A JACK server, or a JACK-compatible backend (e.g. PipeWire's JACK compatibility layer), to run the `oxtt` binary. Not required to build the crate or run `cargo test`.

## Build

```sh
cargo build
cargo build --release
```

## Format and Lint

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Tests

```sh
cargo test --all-targets
```

The suite is organized by module and none of it requires a running JACK server:

- `src/params.rs` — CLI parsing and parameter validation
- `src/dsp/mod.rs` — `OttProcessor` unit tests and processor-level integration tests
- `src/dsp/crossover.rs` — crossover reconstruction and phase-compensator tests
- `src/dsp/compressor.rs` — dual-threshold gain computation tests
- `src/dsp/envelope.rs` — envelope follower and time-scaling tests
- `src/dsp/smooth.rs` — parameter-smoothing tests

See `contracts.md` for what each contract guarantees and which tests verify it.

## Running Locally Without Real Audio Hardware

`oxtt` connects to whichever JACK server is already running, under the client name `oxtt`, and registers four ports (`input_l`, `input_r`, `output_l`, `output_r`) without auto-connecting them. To develop without an audio interface:

```sh
jackd -d dummy &
cargo run --release -- --preset safe-start
```

Connect ports with whichever tool is available in your environment:

- `jack_connect` / `jack_lsp`, if your JACK install ships example clients (Homebrew's `jack2` bottle does not, by default).
- QjackCtl or another GUI patchbay.
- The bundled example helpers, which depend only on the `jack` crate and work in every environment:

  ```sh
  cargo run --example list_ports
  cargo run --example connect_ports -- oxtt:input_l system:capture_1
  ```

## Manual Smoke Test

1. Start `jackd` (a real backend, or `-d dummy`).
2. `cargo run --release -- --preset safe-start`.
3. Confirm 4 ports are registered (`list_ports` example, or `jack_lsp`).
4. Connect ports (`connect_ports` example, `jack_connect`, or a GUI patchbay) and confirm continuous processing at any buffer size.
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

Homebrew's `jack2` bottle does not include the `jack_lsp`/`jack_connect` CLI tools (see `cargo run --example list_ports`/`connect_ports` above for a CLI-tool-free alternative). For interactive verification, QjackCtl is the easiest option:

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
