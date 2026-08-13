#!/usr/bin/env bash
# Build oxtt natively on a Raspberry Pi 5, with the Pi's microarchitecture.
#
# `-C target-cpu=cortex-a76` used to live in `.cargo/config.toml`, scoped to
# `aarch64-unknown-linux-gnu`. It cannot any more: a Bela Gem compiles for that
# same triple and is a Cortex-A53, so a section describing one board would
# silently mis-build the other. Each build exports its own settings instead --
# `CARGO_TARGET_*` outranks the config file. See `.cargo/config.toml` and
# `scripts/bela-build.sh`, which is the other half of the same arrangement.
#
# Run this *on the Pi*: the Pi build is native (host == target), so there is no
# --target and no sysroot. See docs/raspberry-pi/ and docs/development.md.
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/pi-build.sh [--controls] [cargo args...]

Builds oxtt in release mode, natively, tuned for the Pi 5's Cortex-A76.
Any extra arguments are passed through to `cargo build`.

  --controls   Also build the physical control surface (`pi-controls`), which
               needs the SPI ADC and GPIO wiring in docs/raspberry-pi/.

Refuses to run anywhere but an aarch64 Linux host, because the tuning flag
below would otherwise be applied to a build that is not for a Pi at all.
USAGE
}

FEATURES=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help | -h)
      usage
      exit 0
      ;;
    --controls)
      FEATURES=(--features pi-controls)
      shift
      ;;
    *)
      break
      ;;
  esac
done

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "aarch64" ]]; then
  echo "pi-build: this is a native build and must run on the Pi (got $(uname -s)/$(uname -m))" >&2
  exit 1
fi

# The Pi 5 is a Cortex-A76. Set here rather than in `.cargo/config.toml` for
# the reason in the header comment.
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C target-cpu=cortex-a76"

exec cargo build --release --locked "${FEATURES[@]}" "$@"
