# Contracts and Invariants

This is the normative reference for the public DSP API (`src/dsp.rs`, `src/params.rs`), the JACK audio callback (`src/jack_host.rs`), and the physical control surface (`src/control.rs`). It states observable guarantees and real-time requirements; it intentionally does not repeat constructor plumbing, CLI error rendering, lint configuration, or individual test names. `docs/development.md` describes how to run verification, and `docs/decisions/` records the rationale.

## 1. Parameter validation

Parameters are validated before they reach `OttProcessor`; invalid values are rejected, never silently clamped. `OttParams::validate(sample_rate)` additionally validates the host-dependent constraints.

| Parameters | Constraint |
|---|---|
| Input/output gain | finite, `[-24, 24]` dB |
| `depth`, `time`, `upward`, `downward`, and per-band amounts | finite, `[0, 1]` |
| Low crossover | finite, `[40, 2_000]` Hz |
| High crossover | finite, `[400, 16_000]` Hz |
| Crossover pair | `high_hz >= 2 * low_hz` |
| Per-band thresholds | finite, `[-80, 0]` dB and `lower_db < upper_db` |
| Per-band makeup gain | finite, `[-40, 40]` dB |
| Per-band attack/release time | finite, `> 0` ms |
| Sample rate | finite, `[8_000, 384_000]` Hz |
| Crossovers at that sample rate | each `<= 0.45 * sample_rate` |

The crossover-pair and threshold-order invariants hold for every constructed `CrossoverSplit` and `ThresholdRange`. The sample-rate constraint is checked by `OttParams::validate`, because it is supplied by the host and can change while the processor exists.

## 2. Processor lifecycle and updates

- `OttProcessor::new(sample_rate, params)` validates its inputs. On success, parameters start at their targets and the detector starts at 0 dB gain; there is no startup parameter fade.
- `OttProcessor::set_params(params)` validates against the current sample rate. A rejected update leaves the processor unchanged. An accepted update changes only targets: linear parameters use a 20 ms one-pole transition, crossover frequencies use the same transition in log-frequency space, and neither filter nor envelope state is reset.
- `OttProcessor::set_control_snapshot(snapshot)` is the control-surface-only update path. It keeps the same validation and target-only behaviour for time, upward, downward, crossover, and per-band fields, but treats the explicit debounced `bypass_engaged` level as a coordinated transition request. It must not infer bypass from any parameter values.
- `OttProcessor::reset(sample_rate)` validates the most recently accepted targets against the new rate. On success, it rebuilds the processor as if newly constructed with those targets; on failure, it leaves the existing processor unchanged. The host must call it after a sample-rate change.

## 3. Buffer processing

`OttProcessor::process` accepts any slice lengths. If its four input/output slices do not all have the same length, it returns `ProcessError::BufferLengthMismatch` without writing either output. Otherwise it processes and writes exactly that many stereo frames.

For a fixed processor state and input sequence, output is bit-identical regardless of how the input is partitioned across `process` calls.

## 4. Signal invariants

These guarantees hold for every processed sample with validated parameters:

- A non-finite input sample is treated as zero; output samples are finite.
- Dynamic gain is bounded to `[-60, +30]` dB. Silence remains silent, including at maximum upward compression.
- Output is not hard-clipped to `[-1, 1]`.
- Dynamics are fully stereo linked within each band: one gain derived from both channels is applied equally to left and right.
- With `depth = 0`, output is the input-gain/crossover-reconstruction/output-gain path, not a raw bypass. The unprocessed three-band sum is flat within +/-0.1 dB from 20 Hz through `0.45 * sample_rate` for supported, octave-or-wider crossover pairs at 44.1, 48, 96, and 192 kHz.

If crossover or band state becomes non-finite, recovery is limited to that component; an invalid state in one component does not reset unrelated DSP state.

## 5. Crossover transitions

Crossover targets transition in log-frequency space with the 20 ms time constant. Each accepted target change settles in finite time: once the remaining difference is at most `CROSSOVER_SETTLE_CENTS = 0.1` cent, the effective cutoff snaps exactly to its target.

During a transition, coefficients may be updated as needed. Once both cutoffs are settled, coefficients must not be recomputed until a cutoff target or the sample rate changes. Left and right always use the same effective cutoffs, including during transitions.

## 6. Real-time callback

`AudioProcessHandler::process` and its transitive DSP calls must not allocate or free heap memory; acquire or wait on a lock; use a blocking channel operation; perform file or standard-stream I/O; spawn, join, or sleep a thread; panic or unwind; or take more than time proportional to the callback's frame count.

The current JACK callbacks communicate shutdown, sample-rate changes, and xrun diagnostics through atomics. An xrun notification increments a diagnostic counter only; it must not format or emit a log record from the JACK-managed thread. The control surface's path into the callback (section 8) satisfies these same non-blocking, allocation-free requirements, and any further control path must too.

## 7. JACK host lifecycle

`jack_host::run` creates the `oxtt` client with exactly four ports: `input_l`, `input_r`, `output_l`, and `output_r`. It does not hardcode physical port names or auto-connect ports.

The host uses JACK's assigned sample rate and buffer size. It reports connection/setup failures to stderr and returns a non-zero exit status, and it stops safely after JACK shutdown, `SIGINT`, or `SIGTERM`. After a normal stop it returns a run summary to its caller: the number of JACK xrun notifications, and the number of failed control-surface reads if the run had a control surface at all (section 8). The CLI prints those values only when `--report-xruns-on-exit` is requested; normal operation has no mandatory diagnostic output. After a JACK sample-rate notification, the audio callback resets the processor before later processing; a reset failure is contained in the callback rather than causing a panic.

## 8. Control surface

This section applies to a run with a physical control surface attached: six potentiometers and a latching bypass switch (`--controls`, available only in a `pi-controls` build). A run without one behaves exactly as sections 1–7 describe, including its exit report.

Separation from the audio callback:

- Hardware is never read from the audio callback. Potentiometer and switch reads happen on a separate control thread; the callback only ever consumes finished `ControlSnapshot` values: a complete `OttParams` payload plus an explicit debounced bypass level.
- The handoff into the callback is non-blocking, allocation-free, and lock-free in the callback's direction, and takes constant time whether or not a control moved. It satisfies section 6 in full.
- Per cycle the callback takes at most the newest published snapshot. It never drains a backlog; intermediate positions a control passed through are not queued.
- A snapshot is applied strictly after any pending sample-rate reset in the same cycle, so a reset never discards it.
- A snapshot rejected by `set_control_snapshot` leaves the processor unchanged (section 2). The callback neither reports nor retries it; the next accepted snapshot supersedes it.

Failure behaviour:

- Failing to acquire the hardware at startup is fatal: the process reports it to stderr and exits non-zero rather than running with controls that do nothing.
- A hardware read failure once running stops neither audio, nor the control thread, nor the process. It publishes nothing, so the last good snapshot stays in force, and it is counted. Stderr reporting from the control thread is throttled; the exact total is printed after a normal stop, alongside the xrun count and under the same `--report-xruns-on-exit` flag (section 7).

Parameter ownership:

- `depth`, `time`, `upward`, `downward`, input gain, and output gain are owned by the control surface from its first successful read onward. The CLI values for those six describe only the state before that read.
- Every other parameter — the crossover pair, all per-band values — is passed through from the CLI unchanged; no control is wired to it.
- `--preset` therefore selects only the per-band values and the crossover pair. `SafeStart` and `Default` differ only in global `depth` and `output_gain_db` (`docs/decisions/0006-preset-band-values-are-a-compatibility-contract.md`), both of which the control surface owns, so they select identical behaviour under `--controls`. `Riot` differs in its per-band values and crossover pair, so it remains meaningful with controls enabled.
- The two gain controls sweep the whole `[-24, 24]` dB range of section 1, linearly across their travel, so unity gain is at the centre of the rotation.
- Every snapshot the control surface publishes carries a complete `OttParams` that satisfies section 1. The payload always reflects the current pot positions, even while bypassed.

Bypass:

- The bypass is an effect bypass. Its debounced level is transported separately from the complete pot snapshot; a coincidental `depth = 0`, input gain `0 dB`, output gain `0 dB` payload is never a bypass request. The signal stays on the input-gain/crossover-reconstruction/output-gain path (section 4); it is never a raw-signal bypass.
- Because both gains are pinned to unity, the bypassed output cannot exceed the input: bypass is a guaranteed-unity escape, not a gain stage.
- The DSP sequences the three coupled values sample by sample. On engage it holds the active gains, converges depth to zero, then converges both gains to unity. On disengage it holds depth at zero, converges both gains to their latest pot targets, then converges depth to its latest target. Time, upward, downward, crossover, and per-band targets continue through their normal update path throughout.
- Each stage snaps once its remaining error is at most `0.001` depth (about -60 dB wet/dry weight) or `0.01 dB` per gain. These finite thresholds prevent an asymptotic stage from stalling, are below audibility, and add no detector-settle delay: the detector tracks while depth is already zero. A reversal during a moving-depth stage completes only that movement to zero; at the zero-depth waypoint (including either gain stage) the DSP immediately targets the latest requested gain endpoint, without first visiting the opposite one.
- A latest depth-pot value may replace the depth target while the disengaging depth stage is moving. Gain-pot values received in that stage are retained in the complete snapshot but deferred until depth has settled; only then does ordinary active gain smoothing begin. Thus no stage moves depth and gains together.
- DSP regression coverage uses the issue #4 48 kHz / 1 kHz / 0.05-amplitude sine probe, warmed states, and sliding 10 ms RMS windows with a 1 ms hop. Its transition peak allowance is `0.1 dB` above the louder endpoint (leaving only normal crossover reconstruction ripple), and it includes a non-unity input-gain case.
- The switch latches mechanically, so its debounced position *is* the bypass state; there is no press to detect and nothing to toggle. A new position is adopted once it has survived 15 consecutive reads (28 ms at the 2 ms poll interval, so up to 30 ms between the throw and the parameters following), which is what makes the contact bounce of a single throw produce exactly one state change.
- `time`, `upward`, and `downward` keep tracking their controls while bypassed. Disengaging the bypass restores the Depth and gain controls' positions at that moment, not the positions they held when the bypass was engaged.
- A switch resting in the bypassed position when the process starts comes up bypassed: the panel position is the state, from the first reading onward.
- Required hardware follow-up (not yet recorded as passed): on the Pi, use a sustained signal and audio-interface level meter to check both bypass directions with non-unity input/output gain positions.
