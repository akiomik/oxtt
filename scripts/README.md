# Test scripts

These scripts verify `oxtt` on a Raspberry Pi 5 running as a JACK client with a
class-compliant USB audio interface, driven from the Pi host. They depend on a
running JACK server and a physical loopback cable on that USB interface — they
are **not** generic audio tests. They are also **test-only**: neither embeds an
adopted period setting, and neither is meant to launch `oxtt` for normal playing.
For the full procedure, pass criteria, and recorded results, see
[`docs/raspberry-pi/usb-audio-verification.md`](../docs/raspberry-pi/usb-audio-verification.md).

Both require the Pi host setup from
[`docs/raspberry-pi/usb-audio-setup.md`](../docs/raspberry-pi/usb-audio-setup.md): a running JACK
environment, realtime privileges, and release builds of both `oxtt` and the
`oxtt-jack-tools` binaries (`soak_source`, `soak_recorder`, `soak_analyze`) under
`target/release/`. They also need the JACK CLI tools (`jackd`, `jack_lsp`,
`jack_connect`, `jack_cpu_load`, `jack_iodelay`, `jack_samplerate`,
`jack_bufsize`) and `git` on the host, plus tools a standard Raspberry Pi OS
already provides (`sudo`, `journalctl`, `ps`, `awk`, `vcgencmd`, `timeout`). The
host package list is in
[`usb-audio-setup.md` step 3](../docs/raspberry-pi/usb-audio-setup.md). The card name, host name, and
port numbers below are examples from the validation environment — substitute your
own.

## `pi-jack-usb-soak-test.sh` — audio-stability soak

Plays a generated stereo 997 Hz tone through JACK, records the physical loopback
from the explicit capture ports, and rejects an incomplete or silent recording.
In `oxtt` mode it inserts `oxtt` between the source and the playback ports; in
`direct` mode it loops the source straight to the playback ports. Once the graph
is up it also spot-checks the JACK control plane once (`jack_lsp` and
`jack_cpu_load`) and rejects a missing or failed answer. Exits 0 on pass,
non-zero on fail.

Physical wiring: interface Phones L/R → interface Line/Instrument 3/4 inputs.

| Argument | Meaning |
| --- | --- |
| `--card` | ALSA card **name** (e.g. `Pro73056544`), not `hw:0` |
| `--frames` / `--periods` | JACK period geometry (e.g. `128` / `3`) |
| `--mode` | `direct` or `oxtt` |
| `--duration` | Seconds (the soak uses `1800` = 30 minutes) |
| `--capture-left` / `--capture-right` | Capture ports (e.g. `system:capture_3` / `4`) |
| `--playback-left` / `--playback-right` | Playback ports (e.g. `system:playback_3` / `4`) |
| `--output-dir` | Directory for logs, the recorded WAV, and the verdict |

```sh
./scripts/pi-jack-usb-soak-test.sh \
  --card Pro73056544 --frames 128 --periods 3 --mode oxtt --duration 1800 \
  --capture-left system:capture_3 --capture-right system:capture_4 \
  --playback-left system:playback_3 --playback-right system:playback_4 \
  --output-dir <output-dir>
```

On pass, `<output-dir>/result.txt` records `PASS` with the mode and geometry. The
directory also holds the JACK/`oxtt`/recorder/kernel logs, `get_throttled` at
start and end, the recorded WAV, and the automated continuity verdict.

## `pi-jack-usb-latency-test.sh` — round-trip latency

Starts `jackd` at the requested setting, runs `jack_iodelay` across a
single-channel analog loopback for `--duration` seconds, and records the reported
`total roundtrip latency` with the full `jack_iodelay` log and run provenance. In
`oxtt` mode it inserts `oxtt` — with dynamics effectively disabled by `--depth 0`
and 0 dB gains — so the figure includes `oxtt`'s host path.

The latency value itself is **not** judged by the script; a human decides whether
it is acceptable against the target and the feel of playing. The script exits
non-zero only when a trustworthy number could not be produced (JACK did not come
up, ports did not connect, `jack_iodelay` could not lock a value, throttling
occurred, or — in `oxtt` mode — `oxtt` hit an xrun during the measurement).

Physical wiring: interface Phones L → interface Line/Instrument 3 input
(single-channel loopback).

| Argument | Meaning |
| --- | --- |
| `--card` | ALSA card **name** |
| `--frames` / `--periods` | JACK period geometry |
| `--mode` | `direct` or `oxtt` |
| `--duration` | Measurement window in seconds (e.g. `30`) |
| `--playback` | Playback port (e.g. `system:playback_3`) |
| `--capture` | Capture port (e.g. `system:capture_3`) |
| `--output-dir` | Directory for the latency figure, log, and provenance |

```sh
./scripts/pi-jack-usb-latency-test.sh \
  --card Pro73056544 --frames 128 --periods 3 --mode direct --duration 30 \
  --playback system:playback_3 --capture system:capture_3 \
  --output-dir <output-dir>
```

The `final:` line in `<output-dir>/result.txt` holds the most-converged
`total roundtrip latency` in frames and ms; `roundtrip-latency.txt` keeps every
update line so you can confirm the figure converged rather than drifted.
