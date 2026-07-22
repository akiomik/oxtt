#!/usr/bin/env bash
# Run one reproducible Raspberry Pi JACK soak test.

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/pi-soak-test.sh \
    --card CARD --frames FRAMES --periods PERIODS --mode direct|oxtt \
    --duration SECONDS --capture-left PORT --capture-right PORT \
    --playback-left PORT --playback-right PORT --output-dir DIRECTORY \
    [--probe-interval SECONDS]

Physical wiring for this test:
  Babyface Phones L/R -> Babyface Line/Instrument 3/4 inputs.

The script plays a generated stereo 997 Hz tone through JACK, records the
physical loopback from the explicit capture ports, and rejects an incomplete or
silent recording. In oxtt mode it inserts oxtt between the source and the
explicit playback ports. For the recorded Babyface mapping, pass
`system:capture_3`, `system:capture_4`, `system:playback_3`, and
`system:playback_4` respectively.
--probe-interval 0 performs the audio-only trial. A positive value starts the
separate JACK control-plane trial and probes jack_lsp and jack_cpu_load.
USAGE
}

card=''
frames=''
periods=''
mode=''
duration=''
output_dir=''
capture_left=''
capture_right=''
playback_left=''
playback_right=''
probe_interval=0

while (($# > 0)); do
  case "$1" in
    --card) card=${2:?missing value for --card}; shift 2 ;;
    --frames) frames=${2:?missing value for --frames}; shift 2 ;;
    --periods) periods=${2:?missing value for --periods}; shift 2 ;;
    --mode) mode=${2:?missing value for --mode}; shift 2 ;;
    --duration) duration=${2:?missing value for --duration}; shift 2 ;;
    --capture-left) capture_left=${2:?missing value for --capture-left}; shift 2 ;;
    --capture-right) capture_right=${2:?missing value for --capture-right}; shift 2 ;;
    --playback-left) playback_left=${2:?missing value for --playback-left}; shift 2 ;;
    --playback-right) playback_right=${2:?missing value for --playback-right}; shift 2 ;;
    --output-dir) output_dir=${2:?missing value for --output-dir}; shift 2 ;;
    --probe-interval) probe_interval=${2:?missing value for --probe-interval}; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

[[ -n "$card" && -n "$frames" && -n "$periods" && -n "$mode" && -n "$duration" && -n "$capture_left" && -n "$capture_right" && -n "$playback_left" && -n "$playback_right" && -n "$output_dir" ]] || {
  usage >&2
  exit 2
}
[[ "$mode" == direct || "$mode" == oxtt ]] || {
  printf '%s\n' '--mode must be direct or oxtt' >&2
  exit 2
}
[[ "$frames" =~ ^[0-9]+$ && "$periods" =~ ^[0-9]+$ && "$duration" =~ ^[0-9]+$ && "$probe_interval" =~ ^[0-9]+$ ]] || {
  printf '%s\n' 'frames, periods, duration, and probe interval must be non-negative integers' >&2
  exit 2
}
((frames > 0 && periods > 0 && duration > 0)) || {
  printf '%s\n' 'frames, periods, and duration must be positive' >&2
  exit 2
}

for command in jackd jack_lsp jack_connect jack_cpu_load python3 timeout vcgencmd; do
  command -v "$command" >/dev/null || {
    printf 'required command is unavailable: %s\n' "$command" >&2
    exit 1
  }
done
[[ -x target/release/oxtt ]] || {
  printf '%s\n' 'target/release/oxtt is missing; build current main in the distrobox first' >&2
  exit 1
}
[[ -x target/release/examples/soak_source ]] || {
  printf '%s\n' 'target/release/examples/soak_source is missing; build the soak_source example in the distrobox first' >&2
  exit 1
}
[[ -x target/release/examples/soak_recorder ]] || {
  printf '%s\n' 'target/release/examples/soak_recorder is missing; build the soak_recorder example in the distrobox first' >&2
  exit 1
}
[[ "$(git branch --show-current)" == main ]] || {
  printf '%s\n' 'the soak test must run from current main' >&2
  exit 1
}

mkdir -p "$output_dir"
output_dir=$(cd "$output_dir" && pwd)

jackd_pid=''
oxtt_pid=''
recorder_pid=''
source_pid=''
probe_pid=''
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  for pid in "$probe_pid" "$source_pid" "$recorder_pid" "$oxtt_pid" "$jackd_pid"; do
    if [[ -n "$pid" ]]; then
      kill -TERM "$pid" 2>/dev/null || true
    fi
  done
  wait 2>/dev/null || true
  exit "$status"
}
trap cleanup EXIT INT TERM

started_at=$(date -Is)
printf '%s\n' "$started_at" >"$output_dir/started-at.txt"
git rev-parse HEAD | tee "$output_dir/git-revision.txt"
git status --short | tee "$output_dir/git-status.txt"
stat -c '%y %n' target/release/oxtt | tee "$output_dir/oxtt-build.txt"
ulimit -r | tee "$output_dir/ulimit-rtprio.txt"
ulimit -l | tee "$output_dir/ulimit-memlock.txt"
vcgencmd get_throttled | tee "$output_dir/get-throttled-start.txt"
grep -Fx 'throttled=0x0' "$output_dir/get-throttled-start.txt" >/dev/null || {
  printf '%s\n' 'throttling or undervoltage history was present before the test' >&2
  exit 1
}

recording="$output_dir/physical-loopback.wav"

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
ps -T -p "$jackd_pid" -o pid,tid,cls,rtprio,pri,psr,comm | tee "$output_dir/jackd-threads.txt"
for port in "$capture_left" "$capture_right" "$playback_left" "$playback_right"; do
  jack_lsp | grep -Fx "$port" >/dev/null || {
    printf 'required JACK port is unavailable: %s\n' "$port" >&2
    exit 1
  }
done
printf 'capture_left=%s\ncapture_right=%s\nplayback_left=%s\nplayback_right=%s\n' \
  "$capture_left" "$capture_right" "$playback_left" "$playback_right" | tee "$output_dir/physical-ports.txt"

target/release/examples/soak_recorder --duration "$duration" --output "$recording" \
  >"$output_dir/soak-recorder.log" 2>&1 &
recorder_pid=$!
for _ in $(seq 1 100); do
  if jack_lsp | grep -Fx 'soak-recorder:in_1' >/dev/null && jack_lsp | grep -Fx 'soak-recorder:in_2' >/dev/null; then
    break
  fi
  sleep 0.1
done
jack_lsp | grep -Fx 'soak-recorder:in_1' >/dev/null || {
  printf '%s\n' 'soak-recorder did not create input port 1' >&2
  exit 1
}
jack_lsp | grep -Fx 'soak-recorder:in_2' >/dev/null || {
  printf '%s\n' 'soak-recorder did not create input port 2' >&2
  exit 1
}
jack_connect "$capture_left" soak-recorder:in_1
jack_connect "$capture_right" soak-recorder:in_2

target/release/examples/soak_source --duration "$duration" >"$output_dir/soak-source.log" 2>&1 &
source_pid=$!
for _ in $(seq 1 100); do
  if jack_lsp | grep -Fx 'soak-source:out_1' >/dev/null && jack_lsp | grep -Fx 'soak-source:out_2' >/dev/null; then
    break
  fi
  sleep 0.1
done
jack_lsp | grep -Fx 'soak-source:out_1' >/dev/null
jack_lsp | grep -Fx 'soak-source:out_2' >/dev/null

if [[ "$mode" == oxtt ]]; then
  target/release/oxtt --preset safe-start --report-xruns-on-exit >"$output_dir/oxtt.log" 2>&1 &
  oxtt_pid=$!
  for _ in $(seq 1 100); do
    if jack_lsp | grep -Fx 'oxtt:input_l' >/dev/null && jack_lsp | grep -Fx 'oxtt:output_l' >/dev/null; then
      break
    fi
    sleep 0.1
  done
  jack_lsp | grep -Fx 'oxtt:input_l' >/dev/null
  jack_lsp | grep -Fx 'oxtt:input_r' >/dev/null
  jack_lsp | grep -Fx 'oxtt:output_l' >/dev/null
  jack_lsp | grep -Fx 'oxtt:output_r' >/dev/null
  jack_connect soak-source:out_1 oxtt:input_l
  jack_connect soak-source:out_2 oxtt:input_r
  jack_connect oxtt:output_l "$playback_left"
  jack_connect oxtt:output_r "$playback_right"
  ps -T -p "$oxtt_pid" -o pid,tid,cls,rtprio,pri,psr,comm | tee "$output_dir/oxtt-threads.txt"
else
  jack_connect soak-source:out_1 "$playback_left"
  jack_connect soak-source:out_2 "$playback_right"
fi
jack_lsp -c -A | tee "$output_dir/graph.txt"

if ((probe_interval > 0)); then
  (
    success=0
    failure=0
    while kill -0 "$jackd_pid" 2>/dev/null; do
      stamp=$(date -Is)
      if timeout 2s jack_lsp >>"$output_dir/probe-lsp.log" 2>&1 && timeout 2s jack_cpu_load >>"$output_dir/probe-cpu-load.log" 2>&1; then
        success=$((success + 1))
        printf '%s success\n' "$stamp" >>"$output_dir/probe-status.log"
      else
        failure=$((failure + 1))
        printf '%s failure\n' "$stamp" >>"$output_dir/probe-status.log"
      fi
      sleep "$probe_interval"
    done
    printf 'success=%s\nfailure=%s\n' "$success" "$failure" >"$output_dir/probe-summary.txt"
  ) &
  probe_pid=$!
fi

wait "$recorder_pid"
recorder_pid=''
wait "$source_pid"
source_pid=''
if [[ "$mode" == oxtt ]]; then
  kill -TERM "$oxtt_pid"
  wait "$oxtt_pid"
  oxtt_pid=''
fi
if [[ -n "$probe_pid" ]]; then
  kill -TERM "$probe_pid" 2>/dev/null || true
  wait "$probe_pid" 2>/dev/null || true
  probe_pid=''
fi
kill -TERM "$jackd_pid"
wait "$jackd_pid" || true
jackd_pid=''

python3 - "$recording" "$duration" <<'PY'
import struct
import sys
import wave

path, seconds_text = sys.argv[1:]
seconds = int(seconds_text)
sample_rate = 48_000
quiet_threshold = 200
allowed_edge_frames = 2 * sample_rate
allowed_gap_frames = sample_rate // 20

with wave.open(path, 'rb') as wav:
    if (wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) != (2, 2, sample_rate):
        raise SystemExit(f'unexpected WAV format: channels={wav.getnchannels()} width={wav.getsampwidth()} rate={wav.getframerate()}')
    total = wav.getnframes()
    if total < seconds * sample_rate - allowed_edge_frames:
        raise SystemExit(f'recording too short: {total} frames')
    first = None
    last = None
    gap = 0
    max_gap = 0
    index = 0
    while frames := wav.readframes(8192):
        for left, right in struct.iter_unpack('<hh', frames):
            audible = max(abs(left), abs(right)) >= quiet_threshold
            if audible:
                if first is not None:
                    max_gap = max(max_gap, gap)
                else:
                    first = index
                last = index
                gap = 0
            elif first is not None:
                gap += 1
            index += 1
    if first is None:
        raise SystemExit('recording contains no audible test signal')
    if first > allowed_edge_frames:
        raise SystemExit(f'test signal started too late: {first} frames')
    if last is None or last < total - allowed_edge_frames:
        raise SystemExit(f'test signal ended too early: last={last} total={total}')
    if max_gap > allowed_gap_frames:
        raise SystemExit(f'unexpected quiet gap: {max_gap} frames')
    print(f'frames={total}')
    print(f'first_audible_frame={first}')
    print(f'last_audible_frame={last}')
    print(f'max_quiet_gap_frames={max_gap}')
PY

awk 'tolower($0) ~ /(xrun|underrun|overrun)/ { count++ } END { print count + 0 }' "$output_dir/jackd.log" \
  | tee "$output_dir/jack-log-xrun-count.txt"
grep -Fx '0' "$output_dir/jack-log-xrun-count.txt" >/dev/null || {
  printf '%s\n' 'JACK xrun, underrun, or overrun was found' >&2
  exit 1
}
if [[ "$mode" == oxtt ]]; then
  [[ "$(grep -Fxc 'oxtt: xrun_count=0' "$output_dir/oxtt.log" || true)" == 1 ]] || {
    printf '%s\n' 'oxtt xrun summary is missing, duplicated, or nonzero' >&2
    exit 1
  }
fi
[[ "$(grep -Fxc 'dropped_frames=0' "$output_dir/soak-recorder.log" || true)" == 1 ]] || {
  printf '%s\n' 'soak-recorder loss summary is missing or nonzero' >&2
  exit 1
}
if {
  grep -Ein 'xrun|underrun|overrun|ClientDeactivate|ClientCloseAux|Driver is not running|Cannot create new client|socket read failure' \
    "$output_dir/jackd.log" "$output_dir/soak-recorder.log" 2>/dev/null || true
  grep -Ein 'ClientDeactivate|ClientCloseAux|Driver is not running|Cannot create new client|socket read failure' \
    "$output_dir/oxtt.log" 2>/dev/null || true
} | tee "$output_dir/failure-log-matches.txt" | grep -q .; then
  printf '%s\n' 'JACK or client failure pattern was found' >&2
  exit 1
fi
if ((probe_interval > 0)); then
  grep -Fx 'failure=0' "$output_dir/probe-summary.txt" >/dev/null || {
    printf '%s\n' 'a JACK operation probe failed' >&2
    exit 1
  }
  awk '
    /^[0-9]+(\.[0-9]+)?$/ {
      if (count == 0 || $1 > maximum) maximum = $1
      count++
    }
    END {
      if (count == 0) exit 1
      printf "samples=%d\nmaximum=%.6f\n", count, maximum
      exit !(maximum < 50)
    }
  ' "$output_dir/probe-cpu-load.log" | tee "$output_dir/cpu-load-summary.txt" || {
    printf '%s\n' 'JACK CPU load was missing or reached 50 percent' >&2
    exit 1
  }
fi
vcgencmd get_throttled | tee "$output_dir/get-throttled-end.txt"
grep -Fx 'throttled=0x0' "$output_dir/get-throttled-end.txt" >/dev/null
sudo journalctl -k --since "$started_at" | tee "$output_dir/kernel.log"
if grep -Ein 'usb.*(reset|error)|undervoltage|voltage|throttl' "$output_dir/kernel.log" | tee "$output_dir/kernel-failure-matches.txt"; then
  printf '%s\n' 'kernel USB, voltage, or throttling failure was found' >&2
  exit 1
fi

printf 'PASS mode=%s frames=%s periods=%s duration=%s\n' "$mode" "$frames" "$periods" "$duration" | tee "$output_dir/result.txt"
