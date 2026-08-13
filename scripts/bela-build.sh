#!/usr/bin/env bash
# Cross-compile the Bela host binary for a Bela Gem Stereo.
#
# The build settings live here rather than in `.cargo/config.toml` because a
# Raspberry Pi 5 and a Bela Gem compile for the same target triple
# (`aarch64-unknown-linux-gnu`) and want opposite things from it: the Pi is a
# Cortex-A76 built natively on the Pi, the Gem is a Cortex-A53 cross-compiled
# from here with a linker of its own. A `[target.<triple>]` section cannot tell
# them apart, so each build exports its own settings instead --
# `CARGO_TARGET_*` outranks the config file. See `.cargo/config.toml` and
# `scripts/pi-build.sh`, which is the other half of the same arrangement.
#
# For the one-time toolchain and sysroot setup, see
# docs/bela/cross-compile.md.
set -euo pipefail

TARGET=aarch64-unknown-linux-gnu

usage() {
  cat <<'USAGE'
Usage:
  BELA_SYSROOT=/path/to/sysroot scripts/bela-build.sh [cargo args...]

Cross-compiles the `oxtt-bela` binary for a Bela Gem Stereo, in release mode.
Any extra arguments are passed through to `cargo build`.

Required:
  BELA_SYSROOT   A copy of the board's filesystem, synced as described in
                 docs/bela/cross-compile.md. `bela-sys` derives the linker's
                 --sysroot, -B and -Wl,-rpath-link from it.

Optional:
  BELA_LINKER    The cross compiler driver to link with. Defaults to
                 aarch64-unknown-linux-gnu-gcc, which is what the
                 messense/macos-cross-toolchains tap installs. On Debian or
                 Ubuntu this is usually aarch64-linux-gnu-gcc.

The resulting binary is at
target/aarch64-unknown-linux-gnu/release/oxtt-bela; scripts/bela-deploy.sh
copies it to the board and runs it.
USAGE
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

if [[ -z "${BELA_SYSROOT:-}" ]]; then
  echo "bela-build: BELA_SYSROOT is not set; see docs/bela/cross-compile.md" >&2
  exit 1
fi
if [[ ! -d "$BELA_SYSROOT" ]]; then
  echo "bela-build: BELA_SYSROOT is not a directory: $BELA_SYSROOT" >&2
  exit 1
fi

BELA_LINKER="${BELA_LINKER:-aarch64-unknown-linux-gnu-gcc}"
if ! command -v "$BELA_LINKER" >/dev/null 2>&1; then
  echo "bela-build: linker not found: $BELA_LINKER" >&2
  echo "bela-build: install a cross toolchain, or set BELA_LINKER; see docs/bela/cross-compile.md" >&2
  exit 1
fi

# The Gem's PocketBeagle 2 is an AM6254 -- Cortex-A53. Getting this wrong is
# not a build failure but a binary that misbehaves on the board, which is why
# it is set here and not left to a shared config file.
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C target-cpu=cortex-a53"
# Naming the compiler driver directly, with no wrapper script in the path:
# `bela-sys` publishes the sysroot arguments as `links` metadata, `bela`
# relays them, and oxtt's own build.rs turns them into link arguments.
# Cargo also exports this as RUSTC_LINKER, which is how `bela-sys` finds the
# matching C++ compiler for its MIDI shim.
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$BELA_LINKER"
export BELA_SYSROOT

exec cargo build \
  --release \
  --locked \
  --target "$TARGET" \
  --no-default-features \
  --features bela-host \
  --bin oxtt-bela \
  "$@"
