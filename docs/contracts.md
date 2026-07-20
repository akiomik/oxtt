# Contracts and Invariants

This document specifies the preconditions, postconditions, and invariants of `oxtt`'s public DSP API (`src/dsp/mod.rs`, `src/params.rs`) and its real-time audio callback (`src/jack_host.rs`). It is the normative reference for what the implementation guarantees. For *why* a given contract looks the way it does, see `decisions/`.

Each section lists the tests that verify the corresponding contract, by test function name and source file.

## 1. Parameter Validation Contract

Every field is a value object that can only be constructed through its own validated constructor, so single-field range/finiteness checks happen at construction, not deferred. `OttParams::validate(sample_rate)` checks what spans more than one field: the crossover octave separation, each band's threshold ordering, and — the one check that needs `sample_rate`, which isn't known until JACK reports it — the Nyquist-dependent crossover constraint. No field is ever silently clamped — any out-of-contract value is rejected with a `ConfigError` before it can reach `OttProcessor`.

| Field | Constraint | Checked by | Rejected with |
|---|---|---|---|
| `global.input_gain_db`, `output_gain_db` | finite, `[-24, 24]` dB | `IoGain::new` | `String` |
| `global.depth`, `time`, `upward`, `downward` | finite, `[0, 1]` | `NormalizedF32::new` | `String` |
| `global.low_crossover_hz` | finite, `[40, 2000]` Hz | `CrossoverFreqLow::new` | `String` |
| `global.high_crossover_hz` | finite, `[400, 16000]` Hz | `CrossoverFreqHigh::new` | `String` |
| `high_crossover_hz >= 2 * low_crossover_hz` | at least one octave apart | `OttParams::validate` | `ConfigError::CrossoverOctave` |
| `band[i].lower_threshold_db`, `upper_threshold_db` | finite, `[-80, 0]` dB | `Threshold::new` | `String` |
| `band[i].lower_threshold_db < upper_threshold_db` | ascending pair | `OttParams::validate` | `ConfigError::ThresholdOrder` |
| `band[i].up_amount`, `down_amount` | finite, `[0, 1]` | `NormalizedF32::new` | `String` |
| `band[i].makeup_gain_db` | finite, `[-40, 40]` dB | `MakeupGain::new_const` (preset literals only; not runtime-constructible) | compile-time `assert!` |
| `band[i].base_attack_ms`, `base_release_ms` | finite, `> 0` | `PositiveF32::new_const` (preset literals only; not runtime-constructible) | compile-time `assert!` |
| `sample_rate` (via `validate`) | finite, `[8_000, 384_000]` Hz | `OttParams::validate` | `ConfigError::SampleRate` |
| `low_crossover_hz`, `high_crossover_hz` (via `validate`) | `<= 0.45 * sample_rate` | `OttParams::validate` | `ConfigError::CrossoverNyquist` |

Per-field constructors return a plain `String` rather than `ConfigError`: they never take a field-name label (a value object's constructor accepts only data that is part of its own invariant), so the caller assigning the validated value into a named field — `Cli`'s `FromStr` impls today — attaches field context itself. `ConfigError` is reserved for the cross-field and sample-rate-dependent checks in `OttParams::validate`, which does know which fields it's comparing.

Verified by:

- `io_gain_rejects_nan_and_infinite`, `io_gain_rejects_out_of_range`, `normalized_f32_rejects_nan_and_out_of_range`, `crossover_freq_low_rejects_out_of_range`, `crossover_freq_high_rejects_out_of_range`, `threshold_rejects_out_of_range` (`src/params/value.rs`) — per-field checks at construction
- `rejects_crossover_less_than_one_octave_apart`, `accepts_crossover_exactly_one_octave_apart`, `rejects_inverted_thresholds`, `rejects_crossover_above_nyquist_ratio` (`src/params/model.rs`) — cross-field checks in `OttParams::validate`
- `rejects_sample_rate_out_of_range` (`src/params/error.rs`)
- `safe_start_and_default_params_are_valid` (`src/params/preset.rs`) — both shipped presets satisfy every constraint above
- `individual_options_override_preset`, `input_gain_and_output_gain_are_independent`, `rejects_out_of_range_value_at_parse_time`, `from_cli_does_not_enforce_crossover_octave_separation` (`src/cli.rs`) — CLI input goes through the same value objects and `OttParams::validate`

## 2. `OttProcessor` Lifecycle Contract

- **`OttProcessor::new(sample_rate, params)`** fails with `ConfigError` iff `params.validate(sample_rate)` fails. On success, every smoothed value is snapped to its initial target (no startup fade), and every band's envelope state is snapped to its threshold-derived boundary power.
- **`OttProcessor::reset(sample_rate)`** re-validates the *last accepted* `set_params` target against the new `sample_rate`, then rebuilds all filter and envelope state from scratch (equivalent to `new` with the retained target parameters). Must be invoked on every JACK sample-rate change; `OttProcessor` itself does not detect sample-rate changes.
- **`OttProcessor::set_params(params)`** fails with `ConfigError` iff `params.validate(self.sample_rate)` fails; on failure, all existing state and targets are left untouched. On success, only smoothing *targets* change — `current` values converge via the 20 ms one-pole smoothing in `dsp/smooth.rs`. No envelope or filter state is reset.

Verified by:

- `reset_reapplies_last_target_params_without_startup_fade` (`src/dsp/mod.rs`)
- `sample_rate_change_does_not_cause_startup_fade` (`src/dsp/smooth.rs`)
- `init_state_yields_0db_gain`, `init_and_reset_snap_to_threshold_powers` (`src/dsp/compressor.rs`, `src/dsp/envelope.rs`)

## 3. `OttProcessor::process` Buffer Contract

- Precondition: none — any slice lengths are accepted.
- If `input_l.len() != input_r.len() || input_l.len() != output_l.len() || input_l.len() != output_r.len()`: returns `Err(ProcessError::BufferLengthMismatch)` before writing to any output slice.
- Otherwise: processes exactly `input_l.len()` frames, writes every element of `output_l` and `output_r`, and returns `Ok(())`.
- Output is invariant to how a fixed input sequence is partitioned across `process()` calls (single call, 1-sample chunks, or arbitrary irregular chunk sizes all produce bit-identical output).

Verified by:

- `process_rejects_mismatched_buffer_lengths`
- `chunking_does_not_affect_output`
- `result_is_independent_of_chunking` (`src/dsp/smooth.rs`, for the underlying smoothing primitive)

## 4. Numerical Safety Invariants

These hold for every sample processed by `OttProcessor::process`, for any `params` that passed validation:

| Invariant | Mechanism |
|---|---|
| A non-finite input sample is treated as `0.0` | explicit `is_finite()` check in `process_frame` |
| A non-finite output sample is forced to `0.0` | explicit `is_finite()` check after output gain |
| `dynamic_gain_db` is always in `[-60, +30]` dB | `.clamp(MIN_DYNAMIC_GAIN_DB, MAX_DYNAMIC_GAIN_DB)` in `compressor.rs` |
| Non-finite crossover filter state is reset, scoped to the crossover only | `Crossover::is_finite()` / `reset_filter_state()` |
| Non-finite band envelope/filter state is reset, scoped to that band only | `BandProcessor::is_finite()` / `reset_envelope_state()` |
| Output is never hard-clipped to `[-1, 1]` | no clamp applied beyond the finite check |
| Silence stays silent even at maximum upward amount | gain application is purely multiplicative |

Verified by:

- `stays_finite_for_extended_stress_signals` (silence, DC, full-scale sine, impulse, white noise, >= 10 s, `src/dsp/mod.rs`)
- `dc_and_nyquist_neighborhood_do_not_produce_nan_or_inf` (`src/dsp/crossover.rs`)
- `gain_clamp_does_not_exceed_limits` (`src/dsp/compressor.rs`)
- `silence_stays_silent_even_with_max_upward_gain` (`src/dsp/compressor.rs`)

## 5. Crossover Reconstruction Invariant

With `depth = 0`, the sum `low + mid + high` must equal, within floating-point tolerance, "input gain -> LR4 reconstruction -> output gain" computed independently of the dynamics path — dynamics processing must never alter the reconstruction. More generally, the unprocessed 3-band sum must stay flat within +/-0.1 dB from 20 Hz to `0.45 * sample_rate`, for every octave-or-wider crossover pair in the supported range, at 44.1 / 48 / 96 / 192 kHz.

Verified by:

- `depth_zero_matches_pure_crossover_reconstruction` (`src/dsp/mod.rs`)
- `reconstruction_is_flat_at_default_crossover_48khz`, `reconstruction_is_flat_across_sample_rates`, `reconstruction_is_flat_across_representative_crossovers` (`src/dsp/crossover.rs`)
- `phase_compensator_alone_is_unity_gain` (`src/dsp/crossover.rs`)
- `impulse_response_is_finite_and_decays`, `changing_cutoff_target_mid_stream_keeps_bands_in_sync` (`src/dsp/crossover.rs`)

## 6. Real-Time Callback Contract

`AudioProcessHandler::process` (`src/jack_host.rs`), and everything it calls transitively (`OttProcessor::process`, `OttProcessor::reset`), MUST NOT:

- allocate or free heap memory
- acquire or wait on a mutex/rwlock, or perform a blocking channel send/recv
- perform file I/O, or write to stdout/stderr
- spawn or join a thread, or sleep
- panic or unwind
- run in time that is not proportional to the number of samples in the callback

Cross-thread communication into or out of the callback (JACK shutdown, sample-rate change, and any future control-surface parameter updates) MUST use only lock-free primitives: `Arc<AtomicBool>` / `Arc<AtomicU32>` today, and a bounded non-blocking queue for full parameter snapshots in a future control thread.

There is no automated test for this contract today; it is enforced by code review and by the absence of `Mutex`, heap-allocating calls, and I/O calls on the `process` path in `src/jack_host.rs` and `src/dsp/`. `cargo clippy --all-targets -- -D warnings` catches some violations (e.g. some blocking patterns) but not all (e.g. allocation that clippy does not flag by default).

## 7. JACK Host Lifecycle Contract

`jack_host::run` (`src/jack_host.rs`) governs `oxtt`'s behavior as a JACK client, independent of the DSP contracts above:

- Client name is `oxtt`; it registers exactly four ports: `input_l`, `input_r`, `output_l`, `output_r`.
- `oxtt` never hardcodes physical port names and never auto-connects its ports to anything.
- If it cannot connect to the JACK server, it prints the reason to stderr and exits with a non-zero status.
- It stops safely on a JACK shutdown notification, `SIGINT`, or `SIGTERM`.
- It uses the sample rate and buffer size assigned by JACK; it does not request or assume a specific value.
- On a JACK sample-rate-change notification, it calls `OttProcessor::reset` with the new rate before processing further audio (section 2). Reset failure (e.g. the current parameters no longer validate at the new rate) is swallowed rather than propagated, consistent with the real-time callback contract (section 6): the callback never panics.

There is no automated test for this contract; it is exercised by the manual smoke test in `development.md`.
