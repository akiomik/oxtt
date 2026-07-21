# Contracts and Invariants

This is the normative reference for the public DSP API (`src/dsp.rs`, `src/params.rs`) and the JACK audio callback (`src/jack_host.rs`). It states observable guarantees and real-time requirements; it intentionally does not repeat constructor plumbing, CLI error rendering, lint configuration, or individual test names. `docs/development.md` describes how to run verification, and `docs/decisions/` records the rationale.

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

The current JACK callbacks communicate shutdown and sample-rate changes through atomics. Any future control path into the audio callback must preserve these non-blocking, allocation-free requirements.

## 7. JACK host lifecycle

`jack_host::run` creates the `oxtt` client with exactly four ports: `input_l`, `input_r`, `output_l`, and `output_r`. It does not hardcode physical port names or auto-connect ports.

The host uses JACK's assigned sample rate and buffer size. It reports connection/setup failures to stderr and returns a non-zero exit status, and it stops safely after JACK shutdown, `SIGINT`, or `SIGTERM`. After a JACK sample-rate notification, the audio callback resets the processor before later processing; a reset failure is contained in the callback rather than causing a panic.
