#!/usr/bin/env bash
# Copy the cross-compiled Bela host binary to a board and run it.
#
# Does not build: run scripts/bela-build.sh first. Kept separate so that a
# rebuild and a re-run are separate decisions, and so that a stale binary is
# visible as a missing build rather than silently rebuilt over a board that is
# already playing.
set -euo pipefail

TARGET=aarch64-unknown-linux-gnu
BINARY="target/${TARGET}/release/oxtt-bela"

usage() {
  cat <<'USAGE'
Usage:
  scripts/bela-deploy.sh [--host root@bela.local] [--no-run] [-- oxtt args...]

Copies target/aarch64-unknown-linux-gnu/release/oxtt-bela to the board and,
unless --no-run is given, runs it there.

  --host HOST   ssh destination. Default root@bela.local.
  --no-run      Copy only.
  --            Everything after this is passed to oxtt-bela on the board.

The board runs `bela_daemon` on boot, which holds the audio hardware, so this
stops it before starting oxtt. It stays stopped until the board is rebooted or
the service is started again.

ssh is run with -t so that Ctrl-C reaches oxtt rather than the local ssh --
without a tty the signal never arrives and the run has to be killed from
another session.
USAGE
}

HOST=root@bela.local
RUN=1
ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help | -h)
      usage
      exit 0
      ;;
    --host)
      HOST="${2:?--host needs a value}"
      shift 2
      ;;
    --no-run)
      RUN=0
      shift
      ;;
    --)
      shift
      ARGS=("$@")
      break
      ;;
    *)
      echo "bela-deploy: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ ! -f "$BINARY" ]]; then
  echo "bela-deploy: $BINARY not found; run scripts/bela-build.sh first" >&2
  exit 1
fi

echo "bela-deploy: copying $BINARY to $HOST"
scp "$BINARY" "$HOST:"

if [[ "$RUN" -eq 0 ]]; then
  echo "bela-deploy: copied; not running (--no-run)"
  exit 0
fi

# `${ARGS[*]}` rather than a quoted expansion: this is a remote shell command
# line, and the arguments are flags chosen by whoever ran this script.
echo "bela-deploy: stopping bela_daemon and running oxtt-bela ${ARGS[*]:-}"
exec ssh -t "$HOST" "systemctl stop bela_daemon && ./oxtt-bela ${ARGS[*]:-}"
