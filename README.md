# oxtt

[![CI](https://github.com/akiomik/oxtt/actions/workflows/ci.yml/badge.svg)](https://github.com/akiomik/oxtt/actions/workflows/ci.yml)

A 3-band upward/downward multiband compressor for JACK, inspired by Xfer Records OTT, written in Rust.

## Status

**Work in progress.** The end goal is a DIY hardware effector, controlled by physical switches and potentiometers.

`oxtt` runs under two hosts, selected by Cargo feature:

- **JACK** (`jack-host`, on by default) — the `oxtt` binary. Verified on a Raspberry Pi 5 with a class-compliant USB audio interface, and the way the DSP is developed and tested on a desktop.
- **Bela Gem Stereo** (`bela-host`) — the `oxtt-bela` binary, cross-compiled and copied to the board. This is the migration target ADR 0009 left open and [ADR 0011](docs/decisions/0011-bela-gem-stereo-as-the-second-host.md) chose: roughly 1 ms round-trip latency, fanless, and full Linux, so the DSP runs unchanged. It runs on the board at 48 kHz with no underruns and about 19% of one core, but **nothing has been listened to yet** — see [`docs/bela/audio-verification.md`](docs/bela/audio-verification.md) for exactly what is and is not established.

Either host can drive six potentiometers and a latching bypass switch instead of the CLI (`--controls`): the pots drive depth/time/upward/downward and the input/output gains, and the switch bypasses the effect. On the Raspberry Pi that surface has been verified on real hardware, though the two gain pots and the latching switch postdate that verification and are not yet covered by it (see [`docs/raspberry-pi/`](docs/raspberry-pi/)); on Bela the code path runs on the board but nothing is wired to it yet (see [`docs/bela/`](docs/bela/)).

What is not done: the control surface is a breadboard rather than a pedal, and whether a Bela Gem stays cool inside a sealed pedal enclosure — the open question ADR 0009 recorded — has not been measured.

`oxtt` does not aim for binary, preset, or sample-accurate output compatibility with Xfer OTT or any other reference implementation; it is an independent implementation of well-known DSP techniques.

## What It Does

Each stereo input is split into three bands (low / mid / high) using 4th-order Linkwitz-Riley crossovers, and each band gets an independent upward and downward compressor with its own attack/release timing. The three bands are summed back together and, at zero dry/wet depth, reconstruct the input's amplitude response exactly (see [ADR 0001](docs/decisions/0001-phase-compensated-low-branch-crossover.md)).

## Requirements

- Rust, edition 2024 (rustc >= 1.88)
- For the JACK host: a JACK server, or a JACK-compatible backend (e.g. PipeWire's JACK compatibility layer), to run the `oxtt` binary — not required to build the crate or run `cargo test`
- For the Bela host: a Bela Gem Stereo, a cross toolchain, and a sysroot synced off the board, as described in [`docs/bela/cross-compile.md`](docs/bela/cross-compile.md)
- For the Raspberry Pi control surface (`--controls`, the `pi-controls` Cargo feature): a Raspberry Pi 5 wired up as described in [`docs/raspberry-pi/`](docs/raspberry-pi/)

## Build

```sh
cargo build --release
```

That is the JACK host, which is the default. The Bela host is a cross-compile, so it has a script:

```sh
BELA_SYSROOT=~/bela-sysroot scripts/bela-build.sh
scripts/bela-deploy.sh -- --preset safe-start
```

The Raspberry Pi control surface is behind the optional `pi-controls` Cargo feature, off by default because it depends on `rppal` (Linux-only). Build it on the Pi:

```sh
scripts/pi-build.sh --controls
```

See [`docs/development.md`](docs/development.md) for local setup details, including macOS-specific notes, and [`docs/bela/cross-compile.md`](docs/bela/cross-compile.md) for the Bela toolchain.

## Run

```sh
cargo run --release -- --preset safe-start
```

`oxtt` connects to the JACK server under the client name `oxtt` and registers four ports (`input_l`, `input_r`, `output_l`, `output_r`) without auto-connecting them — connect them with `jack_connect`, a GUI patchbay, or the bundled `list_ports`/`connect_ports` helpers in the `oxtt-jack-tools` crate.

Run `cargo run --release -- --help` for the full list of CLI options (gain, depth, time, upward/downward amount, crossover frequencies) and their valid ranges.

**Note:** the `default` and `riot` presets are intentionally strong and can exceed 0 dBFS. Start with `safe-start` and a low monitor level.

## Offline preset comparison

`oxtt-render` is a separate binary in the `oxtt` crate. It runs the same DSP
without starting JACK, measures EBU R128 integrated loudness, and writes a
loudness-matched render for preset comparison:

```sh
cargo run --release --bin oxtt-render -- \
  --input drums.wav --output renders/riot.wav --raw-output renders/riot-raw.wav \
  --preset riot
```

The default target is the input file's integrated loudness; use
`--target-lufs -16` to select a fixed target instead. Its preset and global
parameter flags (`--input-gain`, `--output-gain`, `--depth`, `--time`,
`--upward`, `--downward`, and crossover flags) have exactly the same ranges and
meaning as the JACK client.

The renderer accepts **only stereo 32-bit IEEE-float WAV** input and always
writes that format. It rejects integer WAV and other channel/sample formats
instead of converting, quantizing, or clipping them. It never inserts a
limiter; inspect the reported sample and true peaks before playback.

### `--controls`

Both hosts accept `--controls`, which drives six parameters from a physical control surface instead of the CLI flags: potentiometers for depth/time/upward/downward and the input/output gains, and a latching switch that bypasses the effect. Everything after the hardware read is shared — the jitter filtering, the deadband, the debounce and the mapping to parameters are the same code on both boards ([ADR 0010](docs/decisions/0010-three-layer-control-surface-and-newest-value-handoff.md)).

The flag is opt-in on both, so the same binary still runs off CLI flags alone with nothing wired up.

On a Raspberry Pi 5, with a `pi-controls` build — six pots on an MCP3008 SPI ADC, latching switch on GPIO17:

```sh
scripts/pi-build.sh --controls
./target/release/oxtt --controls
```

The flag does not exist at all without that feature. See [`docs/raspberry-pi/`](docs/raspberry-pi/) for the wiring and setup.

On a Bela Gem Stereo — six pots on `A0`–`A5`, latching switch on `D0`, using the board's own converter and GPIO rather than an external ADC:

```sh
scripts/bela-deploy.sh -- --controls
```

See [`docs/bela/control-surface-setup.md`](docs/bela/control-surface-setup.md) for the wiring, including the one difference from the Pi: Bela's digital pins have no internal pull-up, so the switch needs an external one. [`docs/bela/control-surface-verification.md`](docs/bela/control-surface-verification.md) records what the assembled surface measured — including the idle jitter that gives this board its own deadband ([ADR 0012](docs/decisions/0012-the-jitter-deadband-belongs-to-the-control-source.md)).

## Documentation

Technical documentation lives under `docs/`:

- [`docs/architecture.md`](docs/architecture.md) — component structure, signal flow, state ownership, real-time boundaries
- [`docs/contracts.md`](docs/contracts.md) — normative DSP and real-time audio-callback contracts
- [`docs/decisions/`](docs/decisions/) — design decisions and their rationale (ADRs)
- [`docs/development.md`](docs/development.md) — build, lint, test, and local JACK setup, including macOS notes
- [`docs/raspberry-pi/`](docs/raspberry-pi/) — running and verifying `oxtt` on a Raspberry Pi 5: JACK-over-USB setup, audio-stability and latency verification, and the physical control surface's wiring/SPI setup and hardware verification
- [`docs/bela/`](docs/bela/) — running `oxtt` on a Bela Gem Stereo: cross-compilation setup, the control surface's wiring, and what has been measured on the board
