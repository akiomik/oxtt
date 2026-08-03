# oxtt

[![CI](https://github.com/akiomik/oxtt/actions/workflows/ci.yml/badge.svg)](https://github.com/akiomik/oxtt/actions/workflows/ci.yml)

A 3-band upward/downward multiband compressor for JACK, inspired by Xfer Records OTT, written in Rust.

## Status

**Work in progress.** The end goal is a DIY hardware effector, controlled by physical switches and potentiometers. Today `oxtt` runs as a JACK client whose parameters come from the CLI and, on a Raspberry Pi build with `--controls`, from a physical control surface: six potentiometers on an MCP3008 SPI ADC drive depth/time/upward/downward and the input/output gains, and a latching switch bypasses the effect. Both the JACK client (with a class-compliant USB audio interface) and the control surface have been verified on real hardware, though the two gain pots and the latching switch postdate that verification and are not yet covered by it (see [`docs/raspberry-pi/`](docs/raspberry-pi/)). What is not done: the control surface is a breadboard rather than a pedal, and the hardware platform itself is an open question — the Raspberry Pi 5 runs too hot for a sealed pedal enclosure, which rules out staying on Pi 5 with an I2S HAT; the current leading migration target is **Bela Gem**, with Daisy Seed as an alternative candidate (see [ADR 0009](docs/decisions/0009-hardware-platform-choice-reopened.md)).

`oxtt` does not aim for binary, preset, or sample-accurate output compatibility with Xfer OTT or any other reference implementation; it is an independent implementation of well-known DSP techniques.

## What It Does

Each stereo input is split into three bands (low / mid / high) using 4th-order Linkwitz-Riley crossovers, and each band gets an independent upward and downward compressor with its own attack/release timing. The three bands are summed back together and, at zero dry/wet depth, reconstruct the input's amplitude response exactly (see `docs/decisions/0001-phase-compensated-low-branch-crossover.md`).

## Requirements

- Rust, edition 2024 (rustc >= 1.88)
- A JACK server, or a JACK-compatible backend (e.g. PipeWire's JACK compatibility layer), to run the `oxtt` binary — not required to build the crate or run `cargo test`
- To use the physical control surface (`--controls`, the `pi-controls` Cargo feature): a Raspberry Pi 5 wired up as described in [`docs/raspberry-pi/`](docs/raspberry-pi/)

## Build

```sh
cargo build --release
```

The physical control surface is behind the optional `pi-controls` Cargo feature, off by default because it depends on `rppal` (Linux-only):

```sh
cargo build --release --features pi-controls
```

## Run

```sh
cargo run --release -- --preset safe-start
```

`oxtt` connects to the JACK server under the client name `oxtt` and registers four ports (`input_l`, `input_r`, `output_l`, `output_r`) without auto-connecting them — connect them with `jack_connect`, a GUI patchbay, or the bundled `list_ports`/`connect_ports` helpers in the `oxtt-jack-tools` crate. See `docs/development.md` for local setup details, including macOS-specific notes.

Run `cargo run --release -- --help` for the full list of CLI options (gain, depth, time, upward/downward amount, crossover frequencies) and their valid ranges.

**Note:** the `default` preset is intentionally strong and can exceed 0 dBFS. Start with `safe-start` and a low monitor level.

### `--controls`

On a `pi-controls` build (see Build above) running on a Raspberry Pi 5, pass `--controls` to drive parameters from the physical control surface instead of the CLI flags:

```sh
cargo run --release --features pi-controls -- --controls
```

Six potentiometers on an MCP3008 SPI ADC drive depth/time/upward/downward and the input/output gains, and a latching switch on GPIO17 bypasses the effect. The flag is opt-in and only exists in `pi-controls` builds, so the same binary still runs off CLI flags alone on a Pi with no hardware attached. See [`docs/raspberry-pi/`](docs/raspberry-pi/) for the wiring and setup.

## Documentation

Technical documentation lives under `docs/`:

- [`docs/architecture.md`](docs/architecture.md) — component structure, signal flow, state ownership, real-time boundaries
- [`docs/contracts.md`](docs/contracts.md) — normative DSP and real-time audio-callback contracts
- [`docs/decisions/`](docs/decisions/) — design decisions and their rationale (ADRs)
- [`docs/development.md`](docs/development.md) — build, lint, test, and local JACK setup, including macOS notes
- [`docs/raspberry-pi/`](docs/raspberry-pi/) — running and verifying `oxtt` on a Raspberry Pi 5: JACK-over-USB setup, audio-stability and latency verification, and the physical control surface's wiring/SPI setup and hardware verification
