#!/usr/bin/env bash
# Install, remove or inspect a systemd unit that runs a Bela host at boot.
#
# What it is for is a free USB port. Reaching the board over the network costs
# the USB-A socket a class-compliant MIDI keyboard would otherwise use, and a
# host that starts on its own needs no shell — so installing this and unplugging
# the network adapter is what makes the keyboard fit.
#
# Does not build or copy: run scripts/bela-build.sh and
# scripts/bela-deploy.sh --no-run first, so that what runs at boot is a binary
# somebody chose to put there.
set -euo pipefail

PACKAGE="${BELA_PACKAGE:-oxtt-bela}"
UNIT_DIR=/etc/systemd/system

usage() {
  cat <<'USAGE'
Usage:
  scripts/bela-autostart.sh install [--host root@bela.local] [-- host args...]
  scripts/bela-autostart.sh remove  [--host root@bela.local]
  scripts/bela-autostart.sh status  [--host root@bela.local]

Installs a systemd unit that starts $BELA_PACKAGE at boot with the arguments
after `--`, so the board plays without anybody logging in.

  install       Write the unit, enable it, and start it now.
  remove        Stop, disable and delete the unit. Leaves the binary.
  status        What systemd thinks, and the last lines of the journal.

  --host HOST   ssh destination. Default root@bela.local.
  --            Everything after this becomes the unit's command line.

  BELA_PACKAGE  Which host: oxtt-bela (default) or hyperglare-bela. The same
                variable the build and deploy scripts read.

The unit conflicts with `bela_daemon`, which holds the audio hardware, so
enabling one disables the other for as long as it is installed. `remove` gives
the board back.

Restart is `on-failure` with a delay: a host that cannot open its audio device
because something else has it should stop and say so in the journal, not spin.
USAGE
}

ACTION="${1:-}"
case "$ACTION" in
  install | remove | status) shift ;;
  --help | -h | "")
    usage
    exit 0
    ;;
  *)
    echo "bela-autostart: unknown action: $ACTION" >&2
    usage >&2
    exit 1
    ;;
esac

HOST=root@bela.local
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --host)
      HOST="${2:?--host needs a value}"
      shift 2
      ;;
    --)
      shift
      ARGS=("$@")
      break
      ;;
    *)
      echo "bela-autostart: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

UNIT="${PACKAGE}.service"

case "$ACTION" in
  status)
    exec ssh "$HOST" "systemctl status ${UNIT} --no-pager --lines=20 || true"
    ;;
  remove)
    exec ssh "$HOST" "
      systemctl disable --now ${UNIT} 2>/dev/null || true
      rm -f ${UNIT_DIR}/${UNIT}
      systemctl daemon-reload
      echo 'bela-autostart: removed ${UNIT}; bela_daemon can be started again'
    "
    ;;
esac

# `${ARGS[*]}` rather than a quoted expansion: this becomes a systemd ExecStart
# line, and the arguments are flags chosen by whoever ran this script.
#
# The package name and the arguments are meant to expand here rather than on
# the board — the board is being told what to write, not asked to work it out.
# shellcheck disable=SC2029
echo "bela-autostart: installing ${UNIT} on ${HOST} with ${ARGS[*]:-no arguments}"
# shellcheck disable=SC2029
ssh "$HOST" "
  set -e
  test -x /root/${PACKAGE} || { echo 'bela-autostart: /root/${PACKAGE} is not there; deploy it first' >&2; exit 1; }
  cat > ${UNIT_DIR}/${UNIT} <<UNITFILE
[Unit]
Description=${PACKAGE} (installed by scripts/bela-autostart.sh)
# The daemon holds the audio hardware. One of the two runs, never both.
Conflicts=bela_daemon.service
After=network.target
# Give up rather than restart forever. systemd's default window is 10s, which
# a RestartSec of 5 never fills, so a command line that can never work — a
# flag this host does not have, a MIDI port that is not there — would retry
# until somebody noticed. Five tries at five seconds is 25s, inside 60.
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=simple
ExecStart=/root/${PACKAGE} ${ARGS[*]:-}
# A host that cannot open its audio device should say so once and stop, not
# spin: the journal is the only place anybody will see it.
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
UNITFILE
  systemctl daemon-reload
  systemctl disable --now bela_daemon 2>/dev/null || true
  systemctl enable ${UNIT}
  # Restart rather than \`enable --now\`: a unit that is already running does
  # not pick up a command line that has just been rewritten, so installing
  # over an install would leave the old arguments playing.
  systemctl restart ${UNIT}
  systemctl reset-failed ${UNIT} 2>/dev/null || true
"

# **Whether it stayed up, not whether it started.** A unit whose command line
# cannot work starts, exits, and starts again, so an answer taken immediately
# says `activating` for a service that will never run. This waits for it to
# settle and reports a restart count, which is the difference.
echo "bela-autostart: waiting for ${UNIT} to settle"
for _ in $(seq 30); do
  # shellcheck disable=SC2029
  STATE=$(ssh "$HOST" "systemctl is-active ${UNIT} || true" 2>/dev/null | tr -d '\r')
  # shellcheck disable=SC2029
  RESTARTS=$(ssh "$HOST" "systemctl show ${UNIT} -p NRestarts --value" 2>/dev/null | tr -d '\r')
  if [[ "$STATE" == active && "$RESTARTS" == 0 ]]; then
    echo "bela-autostart: ${UNIT} is running"
    exit 0
  fi
  if [[ "$STATE" == failed ]]; then
    break
  fi
  sleep 1
done

echo "bela-autostart: ${UNIT} did not come up (state ${STATE:-unknown}, ${RESTARTS:-?} restarts)" >&2
# shellcheck disable=SC2029
ssh "$HOST" "journalctl -u ${UNIT} --no-pager --lines=15 -o cat" >&2 || true
echo "bela-autostart: leaving it installed so the journal above stays readable; \
scripts/bela-autostart.sh remove undoes it" >&2
exit 1
