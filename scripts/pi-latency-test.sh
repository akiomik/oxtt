#!/usr/bin/env bash
# Measure one round-trip latency figure with jack_iodelay, in direct or oxtt
# mode, and record it with provenance.
#
# The latency value itself is NOT judged here: a human decides whether it is
# acceptable against the target and the feel of playing. The script only ensures
# the measurement is *valid* -- JACK came up, the loopback ports connected,
# jack_iodelay locked onto a figure, no throttling occurred, and (in oxtt mode)
# oxtt ran the measurement window without an xrun -- and exits non-zero when a
# trustworthy number could not be produced.

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/pi-latency-test.sh \
    --card CARD --frames FRAMES --periods PERIODS --mode direct|oxtt \
    --duration SECONDS --playback PORT --capture PORT --output-dir DIRECTORY

Physical wiring for this test:
  Babyface Phones L -> Babyface Line/Instrument 3 input (single-channel loopback).

The script starts jackd at the requested period setting, runs jack_iodelay
across the physical loopback for --duration seconds, and records the reported
`total roundtrip latency` together with the full jack_iodelay log and run
provenance. In oxtt mode it inserts oxtt -- with dynamics effectively disabled
by --depth 0 and 0 dB gains -- between jack_iodelay and the playback port, so
the figure includes oxtt's host path. For the recorded Babyface mapping, pass
`system:playback_3` and `system:capture_3`.

Judging the number is left to a human; the script exits non-zero only when the
measurement could not be produced or was taken under invalid conditions.
USAGE
}

card=''
frames=''
periods=''
mode=''
duration=''
playback=''
capture=''
output_dir=''

while (($# > 0)); do
  case "$1" in
    --card) card=${2:?missing value for --card}; shift 2 ;;
    --frames) frames=${2:?missing value for --frames}; shift 2 ;;
    --periods) periods=${2:?missing value for --periods}; shift 2 ;;
    --mode) mode=${2:?missing value for --mode}; shift 2 ;;
    --duration) duration=${2:?missing value for --duration}; shift 2 ;;
    --playback) playback=${2:?missing value for --playback}; shift 2 ;;
    --capture) capture=${2:?missing value for --capture}; shift 2 ;;
    --output-dir) output_dir=${2:?missing value for --output-dir}; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

[[ -n "$card" && -n "$frames" && -n "$periods" && -n "$mode" && -n "$duration" && -n "$playback" && -n "$capture" && -n "$output_dir" ]] || {
  usage >&2
  exit 2
}
[[ "$mode" == direct || "$mode" == oxtt ]] || {
  printf '%s\n' '--mode must be direct or oxtt' >&2
  exit 2
}
[[ "$frames" =~ ^[0-9]+$ && "$periods" =~ ^[0-9]+$ && "$duration" =~ ^[0-9]+$ ]] || {
  printf '%s\n' 'frames, periods, and duration must be non-negative integers' >&2
  exit 2
}
((frames > 0 && periods > 0 && duration > 0)) || {
  printf '%s\n' 'frames, periods, and duration must be positive' >&2
  exit 2
}

for command in jackd jack_lsp jack_connect jack_iodelay timeout vcgencmd; do
  command -v "$command" >/dev/null || {
    printf 'required command is unavailable: %s\n' "$command" >&2
    exit 1
  }
done
if [[ "$mode" == oxtt ]]; then
  [[ -x target/release/oxtt ]] || {
    printf '%s\n' 'target/release/oxtt is missing; build current main in the distrobox first' >&2
    exit 1
  }
fi

mkdir -p "$output_dir"
output_dir=$(cd "$output_dir" && pwd)

jackd_pid=''
oxtt_pid=''
iodelay_pid=''
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  for pid in "$iodelay_pid" "$oxtt_pid" "$jackd_pid"; do
    if [[ -n "$pid" ]]; then
      kill -TERM "$pid" 2>/dev/null || true
    fi
  done
  wait 2>/dev/null || true
  exit "$status"
}
trap cleanup EXIT INT TERM

# Provenance. The revision and working-tree state are recorded, not enforced:
# this is a measurement, and the human comparing numbers across configs and
# hardware needs to know exactly which build produced each figure.
git rev-parse HEAD | tee "$output_dir/git-revision.txt"
git status --short | tee "$output_dir/git-status.txt"
if [[ "$mode" == oxtt ]]; then
  stat -c '%y %n' target/release/oxtt | tee "$output_dir/oxtt-build.txt"
fi
vcgencmd get_throttled | tee "$output_dir/get-throttled-start.txt"
grep -Fx 'throttled=0x0' "$output_dir/get-throttled-start.txt" >/dev/null || {
  printf '%s\n' 'throttling or undervoltage history was present before the test' >&2
  exit 1
}

jackd -R -d alsa -d "hw:CARD=$card" -r 48000 -p "$frames" -n "$periods" >"$output_dir/jackd.log" 2>&1 &
jackd_pid=$!

for _ in $(seq 1 100); do
  if jack_lsp >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done
jack_lsp >/dev/null 2>&1 || {
  printf '%s\n' 'JACK server did not become ready' >&2
  exit 1
}
jack_samplerate | tee "$output_dir/jack-samplerate.txt"
jack_bufsize | tee "$output_dir/jack-bufsize.txt"
for port in "$playback" "$capture"; do
  jack_lsp | grep -Fx "$port" >/dev/null || {
    printf 'required JACK port is unavailable: %s\n' "$port" >&2
    exit 1
  }
done
printf 'playback=%s\ncapture=%s\n' "$playback" "$capture" | tee "$output_dir/physical-ports.txt"

if [[ "$mode" == oxtt ]]; then
  # --depth 0 and 0 dB in/out gains make oxtt's dynamics a pass-through, so the
  # figure measures oxtt's host path (buffering) rather than any DSP behaviour.
  target/release/oxtt --preset safe-start --depth 0 --input-gain 0 --output-gain 0 --report-xruns-on-exit \
    >"$output_dir/oxtt.log" 2>&1 &
  oxtt_pid=$!
  for _ in $(seq 1 100); do
    if jack_lsp | grep -Fx 'oxtt:input_l' >/dev/null && jack_lsp | grep -Fx 'oxtt:output_l' >/dev/null; then
      break
    fi
    sleep 0.1
  done
  jack_lsp | grep -Fx 'oxtt:input_l' >/dev/null || {
    printf '%s\n' 'oxtt did not register its input port' >&2
    exit 1
  }
  jack_lsp | grep -Fx 'oxtt:output_l' >/dev/null || {
    printf '%s\n' 'oxtt did not register its output port' >&2
    exit 1
  }
fi

timeout --signal=TERM "${duration}s" jack_iodelay >"$output_dir/jack-iodelay.log" 2>&1 &
iodelay_pid=$!
for _ in $(seq 1 100); do
  if jack_lsp | grep -Fx 'jack_delay:out' >/dev/null && jack_lsp | grep -Fx 'jack_delay:in' >/dev/null; then
    break
  fi
  sleep 0.1
done
jack_lsp | grep -Fx 'jack_delay:out' >/dev/null || {
  printf '%s\n' 'jack_iodelay did not register its output port' >&2
  exit 1
}
jack_lsp | grep -Fx 'jack_delay:in' >/dev/null || {
  printf '%s\n' 'jack_iodelay did not register its input port' >&2
  exit 1
}

if [[ "$mode" == oxtt ]]; then
  jack_connect jack_delay:out oxtt:input_l
  jack_connect oxtt:output_l "$playback"
  jack_connect "$capture" jack_delay:in
else
  jack_connect jack_delay:out "$playback"
  jack_connect "$capture" jack_delay:in
fi
jack_lsp -c -A | tee "$output_dir/graph.txt"

# jack_iodelay is bounded by `timeout`, which ends it with exit status 124 on the
# normal measurement-window timeout; that is expected, so do not judge the run by
# this exit status.
wait "$iodelay_pid" || true
iodelay_pid=''

if [[ "$mode" == oxtt ]]; then
  kill -TERM "$oxtt_pid"
  wait "$oxtt_pid"
  oxtt_pid=''
  # A measurement taken while oxtt was dropping buffers is not trustworthy.
  [[ "$(grep -Fxc 'oxtt: xrun_count=0' "$output_dir/oxtt.log" || true)" == 1 ]] || {
    printf '%s\n' 'oxtt xrun summary is missing, duplicated, or nonzero' >&2
    exit 1
  }
fi
kill -TERM "$jackd_pid"
wait "$jackd_pid" || true
jackd_pid=''

vcgencmd get_throttled | tee "$output_dir/get-throttled-end.txt"
grep -Fx 'throttled=0x0' "$output_dir/get-throttled-end.txt" >/dev/null || {
  printf '%s\n' 'throttling or undervoltage occurred during the test' >&2
  exit 1
}

# jack_iodelay prints a fresh `total roundtrip latency` line on every measurement
# update. If it never locked onto one, no figure was produced and there is
# nothing for a human to judge -- treat that as a failed measurement. Otherwise
# keep every reported figure and surface the last (most converged) one; the human
# reads the full log to confirm the value settled rather than drifting.
grep -i 'total roundtrip latency' "$output_dir/jack-iodelay.log" >"$output_dir/roundtrip-latency.txt" || true
[[ -s "$output_dir/roundtrip-latency.txt" ]] || {
  printf '%s\n' 'jack_iodelay did not report a round-trip latency; check gain staging and the physical loopback' >&2
  exit 1
}
final_latency=$(tail -n 1 "$output_dir/roundtrip-latency.txt")

{
  printf 'MEASURED mode=%s frames=%s periods=%s duration=%s\n' "$mode" "$frames" "$periods" "$duration"
  printf 'final: %s\n' "$final_latency"
} | tee "$output_dir/result.txt"
