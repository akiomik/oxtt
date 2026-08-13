# Building and running oxtt on a Bela Gem Stereo

`oxtt-bela` is cross-compiled on a development machine and copied to the board.
That is the integration model Bela supports — a standalone binary that links
`libbela` and defines the render callbacks — and it is the only one here: the
board is not a build host.

The upstream setup this depends on is
[`bela-rs`'s cross-compilation guide](https://github.com/akiomik/bela-rs/blob/main/docs/cross-compile.md).
This page is what oxtt adds on top of it, and what oxtt does differently.

## What you need once

1. **The Rust target.**

   ```sh
   rustup target add aarch64-unknown-linux-gnu
   ```

2. **A cross toolchain.** The linker is a GNU compiler driver for the target;
   `bela-sys` also compiles a small C++ shim over Bela's `Midi` class, and it
   derives that compiler from the linker so the two stay in one toolchain.

   On macOS, from the [messense/macos-cross-toolchains][tap] tap:

   ```sh
   brew tap messense/macos-cross-toolchains
   brew trust messense/macos-cross-toolchains
   brew install aarch64-unknown-linux-gnu
   ```

   That installs `aarch64-unknown-linux-gnu-gcc`, which is what
   `scripts/bela-build.sh` defaults to. On Debian or Ubuntu the package is
   `gcc-aarch64-linux-gnu` and the driver is `aarch64-linux-gnu-gcc`; pass it
   as `BELA_LINKER`.

3. **A sysroot synced from the board.** The board's headers and libraries, in
   a directory on the development machine — about 800 MB. `bela-rs`'s guide has
   the `rsync` invocation and the list of directories.

   Nothing here assumes where it lives: `BELA_SYSROOT` is read as a plain path,
   so one sysroot can serve several projects and a `bela-rs` checkout that
   already has one needs no second copy. Wherever it lives, keep it out of
   version control — it is the board's filesystem rather than this project's
   source, and `.gitignore` does not cover it.

[tap]: https://github.com/messense/homebrew-macos-cross-toolchains

## Build and run

```sh
export BELA_SYSROOT="$HOME/bela-sysroot"
scripts/bela-build.sh
scripts/bela-deploy.sh -- --preset safe-start
```

A successful build produces
`target/aarch64-unknown-linux-gnu/release/oxtt-bela`, an AArch64 ELF linked
against `libbela.so`. Worth checking once, on the first build, that it is what
it claims to be — a cross-build that quietly produced a host binary would
otherwise only fail on the board:

```sh
file target/aarch64-unknown-linux-gnu/release/oxtt-bela
```

The toolchain's `readelf` — same prefix as its compiler driver — answers the
second half, that the sysroot's `libbela` was found:

```sh
<prefix>-readelf -d target/aarch64-unknown-linux-gnu/release/oxtt-bela | grep NEEDED
```

`bela-deploy.sh` stops `bela_daemon` before starting `oxtt-bela`: the daemon
runs on boot and holds the audio hardware. It also runs `ssh -t`, so that
Ctrl-C reaches `oxtt-bela` rather than the local `ssh`.

**Note:** the `default` and `riot` presets are intentionally strong and can
exceed 0 dBFS. Start with `safe-start` and a low monitor level.

## No linker wrapper

`bela-rs` ships `scripts/aarch64-bela-linker.sh`, a wrapper that adds
`--sysroot`, `-B` and `-Wl,-rpath-link` to the link. oxtt does not use it, and
does not carry a copy of it.

Instead, `bela-sys` publishes those three arguments as Cargo `links` metadata,
`bela` relays them under its own `links` name, and oxtt's `build.rs` turns them
into `cargo::rustc-link-arg`. Cargo passes `DEP_*` metadata only to an
*immediate* dependent's build script, which is the whole reason oxtt needs a
`build.rs` at all. The linker named in `scripts/bela-build.sh` is then the
compiler driver itself, with nothing in front of it.

## Why the linker is set in a script and not `.cargo/config.toml`

`bela-rs`'s guide puts the linker in `.cargo/config.toml`:

```toml
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-unknown-linux-gnu-gcc"
```

oxtt cannot. A Raspberry Pi 5 build compiles for the same triple, natively on
the Pi, and wants a plain native `cc` and `-C target-cpu=cortex-a76`; a Bela
Gem is a Cortex-A53 and wants the cross driver above. One
`[target.<triple>]` section cannot describe both, and whichever it described
would silently mis-build the other — a Pi binary linked with a cross toolchain,
or a Bela binary tuned for the wrong microarchitecture.

So each build exports its own settings, from the script that knows which board
it is for:

| | `scripts/pi-build.sh` | `scripts/bela-build.sh` |
| --- | --- | --- |
| `CARGO_TARGET_..._RUSTFLAGS` | `-C target-cpu=cortex-a76` | `-C target-cpu=cortex-a53` |
| `CARGO_TARGET_..._LINKER` | unset (native) | the cross driver |
| `--target` | none (host == target) | `aarch64-unknown-linux-gnu` |

`CARGO_TARGET_*` outranks the config file, and Cargo exports the linker it
resolves as `RUSTC_LINKER`, which is how `bela-sys` finds the matching C++
compiler for its MIDI shim. `.cargo/config.toml` is left with no
`[target.aarch64-unknown-linux-gnu]` section at all, which is also what makes a
bare `cargo check --target aarch64-unknown-linux-gnu` a configuration that
needs no linker and no sysroot — see below.

## Type-checking without a board

`cargo clippy --target aarch64-unknown-linux-gnu` does not link, so it needs
neither `libbela` nor a sysroot, and it is the only way to compile the half of
the host behind `cfg(bela_device)`:

```sh
cargo clippy -p oxtt --no-default-features --features bela-host \
  --all-targets --target aarch64-unknown-linux-gnu -- -D warnings
```

CI runs exactly this. Everything *not* behind that cfg — the application type,
the control conversion, and their tests — is ordinary code that builds and runs
on the development machine, because `bela`'s device code is behind a
`bela_device` cfg its build script sets only for aarch64 Linux:

```sh
cargo clippy -p oxtt --no-default-features --features bela-host --all-targets -- -D warnings
cargo test  -p oxtt --no-default-features --features bela-host --all-targets
```

What cannot be checked off the board is the panic-free proof
([contracts.md §6](../contracts.md#6-real-time-callback)): it needs
`cargo test --release`, which needs to run the tests, which needs the target.
It is established on x86-64 instead, as it already is for the Raspberry Pi
build — the proof is a property of the source and the optimiser rather than of
the instruction set.

## Landmines

Measured on the board and recorded in
[`bela-rs`'s board facts](https://github.com/akiomik/bela-rs/blob/main/docs/board-facts.md);
listed here because each of them ends a run rather than reporting an error.

- **A failed initialisation poisons the process.** Once `Bela_initAudio` has
  failed, every later attempt in the same process fails too. oxtt refuses
  everything it can before initialisation ([contracts.md §9](../contracts.md#9-bela-host-lifecycle))
  so that a mistake in the arguments is a message rather than a dead process.
- **`--period 1` and `--period 3` hang the PRU** with eight analog inputs
  configured, and libbela exits the process from inside itself. The default of
  16 is also the smallest size worth asking for.
- **CPU monitoring refuses a period above 128.** `--report-cpu` with
  `--period 256` fails to start. It is off by default for that reason.
- **Sample rates of 108 kHz and above abort from inside the codec.** oxtt asks
  for 48 kHz; `--sample-rate` will pass whatever you give it.
- **Analog outputs do not exist on a Gem Stereo.** oxtt never asks for a
  different number of analog outputs than inputs, because a mismatch fails
  initialisation outright.
