# oxtt

[![CI](https://github.com/akiomik/oxtt/actions/workflows/ci.yml/badge.svg)](https://github.com/akiomik/oxtt/actions/workflows/ci.yml)

A 3-band upward/downward multiband compressor for JACK, inspired by Xfer Records OTT, written in Rust.

## Status

**Work in progress.** The end goal is a DIY hardware effector: `oxtt` running on a Raspberry Pi 5 with a USB audio interface, controlled by physical switches and potentiometers. Right now, development is at the PC stage — `oxtt` runs as a JACK client with CLI-only parameters, and Raspberry Pi / hardware-control support does not exist yet.

`oxtt` does not aim for binary, preset, or sample-accurate output compatibility with Xfer OTT or any other reference implementation; it is an independent implementation of well-known DSP techniques.

## What It Does

Each stereo input is split into three bands (low / mid / high) using 4th-order Linkwitz-Riley crossovers, and each band gets an independent upward and downward compressor with its own attack/release timing. The three bands are summed back together and, at zero dry/wet depth, reconstruct the input's amplitude response exactly (see `docs/decisions/0001-phase-compensated-low-branch-crossover.md`).

## Requirements

- Rust, edition 2024 (rustc >= 1.85)
- A JACK server, or a JACK-compatible backend (e.g. PipeWire's JACK compatibility layer)

## Build

```sh
cargo build --release
```

## Run

```sh
cargo run --release -- --preset safe-start
```

`oxtt` connects to the JACK server under the client name `oxtt` and registers four ports (`input_l`, `input_r`, `output_l`, `output_r`) without auto-connecting them — connect them with `jack_connect`, a GUI patchbay, or the bundled `list_ports`/`connect_ports` examples. See `docs/development.md` for local setup details, including macOS-specific notes.

Run `cargo run --release -- --help` for the full list of CLI options (gain, depth, time, upward/downward amount, crossover frequencies) and their valid ranges.

**Note:** the `default` preset is intentionally strong and can exceed 0 dBFS. Start with `safe-start` and a low monitor level.

## Documentation

Technical documentation lives under `docs/`:

- [`docs/architecture.md`](docs/architecture.md) — component structure, signal flow, state ownership, real-time boundaries
- [`docs/contracts.md`](docs/contracts.md) — preconditions, postconditions, and invariants of the public API and the audio callback, with links to the tests that verify them
- [`docs/decisions/`](docs/decisions/) — design decisions and their rationale (ADRs)
- [`docs/development.md`](docs/development.md) — build, lint, test, and local JACK setup, including macOS notes
