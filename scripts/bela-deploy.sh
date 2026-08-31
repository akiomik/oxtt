#!/usr/bin/env bash
# Copy the cross-compiled Bela host binary to a board and run it.
#
# Does not build: run scripts/bela-build.sh first. Kept separate so that a
# rebuild and a re-run are separate decisions, and so that a stale binary is
# visible as a missing build rather than silently rebuilt over a board that is
# already playing.
set -euo pipefail

TARGET=aarch64-unknown-linux-gnu
# Which host to deploy. Matches scripts/bela-build.sh's BELA_PACKAGE, so a
# build and a deploy of the same effect need the same one variable set.
PACKAGE="${BELA_PACKAGE:-oxtt-bela}"
BINARY="target/${TARGET}/release/${PACKAGE}"

usage() {
  cat <<'USAGE'
Usage:
  scripts/bela-deploy.sh [--host root@bela.local] [--no-run] [-- host args...]

Copies target/aarch64-unknown-linux-gnu/release/$BELA_PACKAGE to the board
and, unless --no-run is given, runs it there.

  --host HOST   ssh destination. Default root@bela.local.
  --no-run      Copy only.
  --            Everything after this is passed to the binary on the board.

If a `$BELA_PACKAGE.service` unit is running — see scripts/bela-autostart.sh —
it is stopped for the copy and started again afterwards, because a binary a
service is running cannot be written over.

  BELA_PACKAGE  Which host to deploy: oxtt-bela (default) or
                hyperglare-bela. The same variable scripts/bela-build.sh
                reads.

The board runs `bela_daemon` on boot, which holds the audio hardware, so this
stops it before starting the host. It stays stopped until the board is rebooted or
the service is started again.

ssh is run with -t so that Ctrl-C reaches the host rather than the local ssh --
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

# A binary a service is running cannot be written over, and `scp` says so as
# "failed to upload" rather than as anything about the service. Stopped here
# and put back afterwards, so a deploy leaves the board as it found it.
#
# The unit name expands here rather than on the board: the board is being told
# which service, not asked to work it out.
UNIT="${PACKAGE}.service"
WAS_RUNNING=0
# shellcheck disable=SC2029
if ssh "$HOST" "systemctl is-active --quiet ${UNIT}" 2>/dev/null; then
  echo "bela-deploy: ${UNIT} is running; stopping it to write over its binary"
  # shellcheck disable=SC2029
  ssh "$HOST" "systemctl stop ${UNIT}"
  WAS_RUNNING=1
fi

echo "bela-deploy: copying $BINARY to $HOST"
scp "$BINARY" "$HOST:"

if [[ "$WAS_RUNNING" -eq 1 ]]; then
  echo "bela-deploy: starting ${UNIT} again"
  # shellcheck disable=SC2029
  ssh "$HOST" "systemctl start ${UNIT}"
fi

if [[ "$RUN" -eq 0 ]]; then
  echo "bela-deploy: copied; not running (--no-run)"
  exit 0
fi

# `${ARGS[*]}` rather than a quoted expansion: this is a remote shell command
# line, and the arguments are flags chosen by whoever ran this script.
echo "bela-deploy: stopping bela_daemon and running $PACKAGE ${ARGS[*]:-}"
exec ssh -t "$HOST" "systemctl stop bela_daemon && ./${PACKAGE} ${ARGS[*]:-}"
