#!/usr/bin/env bash
# Capture and reduce idle jitter for all six control-surface channels.
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/pi-idle-jitter.sh [--readings N]

Captures N readings (default 300) per channel from oxtt-pi-tools and reduces
them to n/min/max/spread/sd per channel: Depth, Time, Upward, Downward,
InputGain, OutputGain. Counts are raw MCP3008 counts out of 1023.

Move all six pots to the position being measured (e.g. full travel or mid
travel) and leave them completely untouched before running this -- the
result is only meaningful for a motionless pot. Run it again after
repositioning the pots to capture a different position; nothing here
automates the physical move.

For what this measures, why, and the recorded results, see
docs/raspberry-pi/control-surface-verification.md.
USAGE
}

readings=300

while (($# > 0)); do
  case "$1" in
    --readings) readings=${2:?missing value for --readings}; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

[[ "$readings" =~ ^[0-9]+$ ]] || {
  printf '%s\n' '--readings must be a non-negative integer' >&2
  exit 2
}
((readings > 0)) || {
  printf '%s\n' '--readings must be positive' >&2
  exit 2
}

# oxtt-pi-tools prints a banner line before the first reading, so one extra
# line is dropped along with it.
head_lines=$((readings + 1))

cargo run --release -p oxtt-pi-tools \
  | head -n "$head_lines" | tail -n "$readings" \
  | awk -v expected_n="$readings" '
      {
        for (i = 1; i <= NF; i++) {
          if (split($i, kv, "=") == 2 && kv[1] ~ /^(Depth|Time|Upward|Downward|InputGain|OutputGain)$/) {
            c = kv[1]; v = kv[2] + 0
            n[c]++; sum[c] += v; sq[c] += v * v
            if (n[c] == 1 || v < min[c]) min[c] = v
            if (n[c] == 1 || v > max[c]) max[c] = v
          }
        }
      }
      END {
        split("Depth Time Upward Downward InputGain OutputGain", order, " ")
        status = 0
        for (i = 1; i <= 6; i++) {
          c = order[i]
          if (n[c] != expected_n) {
            printf "%-11s only %d of %d expected readings arrived\n", c, n[c] + 0, expected_n > "/dev/stderr"
            status = 1
            continue
          }
          mean = sum[c] / n[c]
          printf "%-11s n=%3d min=%4d max=%4d spread=%3d sd=%.2f\n",
                 c, n[c], min[c], max[c], max[c] - min[c], sqrt(sq[c] / n[c] - mean * mean)
        }
        exit status
      }'
