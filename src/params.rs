//! Interprets CLI values, and handles parameter validation, normalization, and presets.
//!
//! Ranges and invariants follow `docs/contracts.md` §1.
//!
//! Every value object below exposes at most two constructors:
//! - `new(...) -> Result<Self, ConfigError>`: the only public, runtime-checked way to
//!   build one from an untrusted value (CLI input, or a library caller). Only exists
//!   where such a construction path actually exists.
//! - `new_const(...) -> Self`: `pub(crate)`, for the fixed preset literals only.
//!   Asserts the same invariants, so an invalid literal fails to compile rather than
//!   slipping past a forgotten runtime check.
//!
//! Once constructed, every field of `OttParams` is therefore guaranteed valid on its
//! own; the only thing that still needs checking against external state is the
//! sample-rate-dependent Nyquist constraint, handled by `OttParams::validate`.

use std::str::FromStr;

use thiserror::Error;

/// Lower bound of the allowed sample rate range (docs/contracts.md §1).
pub const MIN_SAMPLE_RATE_HZ: f32 = 8_000.0;
/// Upper bound of the allowed sample rate range (docs/contracts.md §1).
pub const MAX_SAMPLE_RATE_HZ: f32 = 384_000.0;

/// Upper-bound coefficient on the Nyquist side that crossover frequencies must respect (docs/contracts.md §1).
pub const CROSSOVER_NYQUIST_RATIO: f32 = 0.45;

/// A normalized fraction in `0.0..=1.0` (docs/contracts.md §1).
///
/// Shared by the dry/wet mix, the attack/release time multiplier, the
/// upward/downward multipliers, and each band's compression amounts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitInterval(f32);

impl UnitInterval {
    /// Validates and wraps `value`.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `value` is not finite or falls outside `0.0..=1.0`.
    pub fn new(field: &'static str, value: f32) -> Result<Self, ConfigError> {
        check_range(field, value, 0.0, 1.0)?;
        Ok(Self(value))
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(
            value.is_finite() && value >= 0.0 && value <= 1.0,
            "UnitInterval literal out of range"
        );
        Self(value)
    }

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A band's downward/upward compression threshold pair, in dB (docs/contracts.md §1).
///
/// Both bounds must lie in `-80.0..=0.0`, and `lower_db` must be less than `upper_db`.
/// Only ever constructed from preset literals (ADR 0006); not CLI-configurable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThresholdRange {
    lower_db: f32,
    upper_db: f32,
}

impl ThresholdRange {
    /// For preset literals only. Panics (at compile time, in a `const` context) if the pair is invalid.
    pub(crate) const fn new_const(lower_db: f32, upper_db: f32) -> Self {
        assert!(
            lower_db.is_finite() && lower_db >= -80.0 && lower_db <= 0.0,
            "ThresholdRange lower_db literal out of range"
        );
        assert!(
            upper_db.is_finite() && upper_db >= -80.0 && upper_db <= 0.0,
            "ThresholdRange upper_db literal out of range"
        );
        assert!(
            lower_db < upper_db,
            "ThresholdRange lower_db must be less than upper_db"
        );
        Self { lower_db, upper_db }
    }

    /// Returns the downward-compression threshold in dB.
    #[must_use]
    pub const fn lower_db(self) -> f32 {
        self.lower_db
    }

    /// Returns the upward-compression threshold in dB.
    #[must_use]
    pub const fn upper_db(self) -> f32 {
        self.upper_db
    }
}

/// A low/mid and mid/high crossover frequency pair, in Hz (docs/contracts.md §1).
///
/// `low_hz` must lie in `40.0..=2000.0`, `high_hz` must lie in `400.0..=16000.0`,
/// and `high_hz` must be at least one octave above `low_hz`. The Nyquist-relative
/// limit depends on the sample rate, so it's checked separately by `OttParams::validate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossoverPair {
    low_hz: f32,
    high_hz: f32,
}

impl CrossoverPair {
    /// Validates and wraps `low_hz`/`high_hz`.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if either frequency is not finite, falls outside its
    /// allowed range, or `high_hz` is less than one octave above `low_hz`.
    pub fn new(low_hz: f32, high_hz: f32) -> Result<Self, ConfigError> {
        check_range("low_crossover_hz", low_hz, 40.0, 2000.0)?;
        check_range("high_crossover_hz", high_hz, 400.0, 16000.0)?;
        if high_hz < 2.0 * low_hz {
            return Err(ConfigError::CrossoverOctave { low_hz, high_hz });
        }
        Ok(Self { low_hz, high_hz })
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if the pair is invalid.
    pub(crate) const fn new_const(low_hz: f32, high_hz: f32) -> Self {
        assert!(
            low_hz.is_finite() && low_hz >= 40.0 && low_hz <= 2000.0,
            "CrossoverPair low_hz literal out of range"
        );
        assert!(
            high_hz.is_finite() && high_hz >= 400.0 && high_hz <= 16000.0,
            "CrossoverPair high_hz literal out of range"
        );
        assert!(
            high_hz >= 2.0 * low_hz,
            "CrossoverPair literal violates octave separation"
        );
        Self { low_hz, high_hz }
    }

    /// Returns the low/mid crossover frequency in Hz.
    #[must_use]
    pub const fn low_hz(self) -> f32 {
        self.low_hz
    }

    /// Returns the mid/high crossover frequency in Hz.
    #[must_use]
    pub const fn high_hz(self) -> f32 {
        self.high_hz
    }
}

/// A gain value in dB, range `-24.0..=24.0` (docs/contracts.md §1).
///
/// Used for the pre-split and post-sum gains, which are CLI-configurable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainDb(f32);

impl GainDb {
    /// Validates and wraps `value`.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `value` is not finite or falls outside `-24.0..=24.0`.
    pub fn new(field: &'static str, value: f32) -> Result<Self, ConfigError> {
        check_range(field, value, -24.0, 24.0)?;
        Ok(Self(value))
    }

    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(
            value.is_finite() && value >= -24.0 && value <= 24.0,
            "GainDb literal out of range"
        );
        Self(value)
    }

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A band's makeup gain in dB, range `-40.0..=40.0` (docs/contracts.md §1).
///
/// Only ever constructed from preset literals (ADR 0006); not CLI-configurable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MakeupGainDb(f32);

impl MakeupGainDb {
    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(
            value.is_finite() && value >= -40.0 && value <= 40.0,
            "MakeupGainDb literal out of range"
        );
        Self(value)
    }

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A positive duration in milliseconds (docs/contracts.md §1).
///
/// Used for each band's base attack/release time at `time = 0.5`. Only ever
/// constructed from preset literals (ADR 0006); not CLI-configurable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositiveMs(f32);

impl PositiveMs {
    /// For preset literals only. Panics (at compile time, in a `const` context) if `value` is invalid.
    pub(crate) const fn new_const(value: f32) -> Self {
        assert!(
            value.is_finite() && value > 0.0,
            "PositiveMs literal must be positive"
        );
        Self(value)
    }

    /// Returns the wrapped value.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Global parameters shared across all bands (docs/contracts.md §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalParams {
    /// Pre-split gain in dB.
    pub input_gain_db: GainDb,
    /// Post-sum gain in dB.
    pub output_gain_db: GainDb,
    /// Dry/wet mix.
    pub depth: UnitInterval,
    /// Attack/release time multiplier.
    pub time: UnitInterval,
    /// Upward compression amount multiplier.
    pub upward: UnitInterval,
    /// Downward compression amount multiplier.
    pub downward: UnitInterval,
    /// Low/mid and mid/high crossover frequency pair.
    pub crossovers: CrossoverPair,
}

/// Per-band parameters (docs/contracts.md §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandParams {
    /// Downward/upward compression threshold pair in dB.
    pub thresholds: ThresholdRange,
    /// Upward compression amount.
    pub up_amount: UnitInterval,
    /// Downward compression amount.
    pub down_amount: UnitInterval,
    /// Makeup gain in dB.
    pub makeup_gain_db: MakeupGainDb,
    /// Attack time in ms at `time = 0.5`.
    pub base_attack_ms: PositiveMs,
    /// Release time in ms at `time = 0.5`.
    pub base_release_ms: PositiveMs,
}

/// All parameters used to construct or update an `OttProcessor`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OttParams {
    /// Parameters shared across all bands.
    pub global: GlobalParams,
    /// Per-band parameters, indexed by `BAND_LOW`/`BAND_MID`/`BAND_HIGH`.
    pub bands: [BandParams; 3],
}

/// Index of the low band within `OttParams::bands`.
pub const BAND_LOW: usize = 0;
/// Index of the mid band within `OttParams::bands`.
pub const BAND_MID: usize = 1;
/// Index of the high band within `OttParams::bands`.
pub const BAND_HIGH: usize = 2;

/// Validation error when constructing or updating parameters (docs/contracts.md §1).
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ConfigError {
    /// A field's value was NaN or infinite.
    #[error("{field} must be finite, got {value}")]
    NotFinite {
        /// Name of the offending field.
        field: &'static str,
        /// The offending value.
        value: f32,
    },
    /// A field's value fell outside its allowed range.
    #[error("{field} must be in [{min}, {max}], got {value}")]
    OutOfRange {
        /// Name of the offending field.
        field: &'static str,
        /// Lower bound of the allowed range, inclusive.
        min: f32,
        /// Upper bound of the allowed range, inclusive.
        max: f32,
        /// The offending value.
        value: f32,
    },
    /// `high_crossover_hz` was less than one octave above `low_crossover_hz`.
    #[error(
        "high_crossover_hz ({high_hz}) must be at least one octave above low_crossover_hz ({low_hz})"
    )]
    CrossoverOctave {
        /// The offending `low_crossover_hz`.
        low_hz: f32,
        /// The offending `high_crossover_hz`.
        high_hz: f32,
    },
    /// A crossover frequency exceeded the Nyquist-relative limit at the current sample rate.
    #[error("{field} ({value}) exceeds {max} at the current sample rate")]
    CrossoverNyquist {
        /// Name of the offending field.
        field: &'static str,
        /// The offending value.
        value: f32,
        /// The limit the value exceeded.
        max: f32,
    },
    /// The sample rate was outside `MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ` or not finite.
    #[error(
        "sample_rate must be finite and in [{MIN_SAMPLE_RATE_HZ}, {MAX_SAMPLE_RATE_HZ}], got {value}"
    )]
    SampleRate {
        /// The offending sample rate.
        value: f32,
    },
}

const fn check_finite(field: &'static str, value: f32) -> Result<(), ConfigError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ConfigError::NotFinite { field, value })
    }
}

fn check_range(field: &'static str, value: f32, min: f32, max: f32) -> Result<(), ConfigError> {
    check_finite(field, value)?;
    if value < min || value > max {
        Err(ConfigError::OutOfRange {
            field,
            min,
            max,
            value,
        })
    } else {
        Ok(())
    }
}

/// Validates the sample rate alone (docs/contracts.md §1).
///
/// # Errors
///
/// Returns `ConfigError::SampleRate` if `sample_rate` is not finite or falls
/// outside `MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ`.
pub fn validate_sample_rate(sample_rate: f32) -> Result<(), ConfigError> {
    if sample_rate.is_finite() && (MIN_SAMPLE_RATE_HZ..=MAX_SAMPLE_RATE_HZ).contains(&sample_rate) {
        Ok(())
    } else {
        Err(ConfigError::SampleRate { value: sample_rate })
    }
}

impl OttParams {
    /// Validates the sample-rate-dependent Nyquist constraint.
    ///
    /// Every other invariant is guaranteed by construction: each field is a
    /// value object that can only be built through its own validated
    /// constructor (docs/contracts.md §1). This is the one check that needs
    /// `sample_rate`, which isn't known until JACK reports it.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `sample_rate` is invalid, or a crossover
    /// frequency exceeds the Nyquist-relative limit.
    pub fn validate(&self, sample_rate: f32) -> Result<(), ConfigError> {
        validate_sample_rate(sample_rate)?;

        let nyquist_limit = CROSSOVER_NYQUIST_RATIO * sample_rate;
        let g = &self.global;
        if g.crossovers.low_hz() > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "low_crossover_hz",
                value: g.crossovers.low_hz(),
                max: nyquist_limit,
            });
        }
        if g.crossovers.high_hz() > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "high_crossover_hz",
                value: g.crossovers.high_hz(),
                max: nyquist_limit,
            });
        }

        Ok(())
    }
}

/// Startup presets (docs/contracts.md §1, ADR 0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    /// Conservative output level, suitable for a first listen (docs/contracts.md §1).
    #[default]
    SafeStart,
    /// Intentionally strong preset that can exceed 0 dBFS (ADR 0006).
    Default,
}

impl Preset {
    // Band values are fixed as a compatibility target for the `Default` preset, per ADR 0006.
    const LOW_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(-35.0, -28.0),
        up_amount: UnitInterval::new_const(0.800),
        down_amount: UnitInterval::new_const(0.900),
        makeup_gain_db: MakeupGainDb::new_const(16.3),
        base_attack_ms: PositiveMs::new_const(2.8),
        base_release_ms: PositiveMs::new_const(40.0),
    };
    const MID_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(-36.0, -25.0),
        up_amount: UnitInterval::new_const(0.800),
        down_amount: UnitInterval::new_const(0.857),
        makeup_gain_db: MakeupGainDb::new_const(11.7),
        base_attack_ms: PositiveMs::new_const(1.4),
        base_release_ms: PositiveMs::new_const(28.0),
    };
    const HIGH_BAND: BandParams = BandParams {
        thresholds: ThresholdRange::new_const(-35.0, -30.0),
        up_amount: UnitInterval::new_const(0.800),
        down_amount: UnitInterval::new_const(1.000),
        makeup_gain_db: MakeupGainDb::new_const(16.3),
        base_attack_ms: PositiveMs::new_const(0.7),
        base_release_ms: PositiveMs::new_const(15.0),
    };

    const fn bands() -> [BandParams; 3] {
        [Self::LOW_BAND, Self::MID_BAND, Self::HIGH_BAND]
    }

    /// Returns the complete parameters for this preset.
    #[must_use]
    pub const fn params(self) -> OttParams {
        let bands = Self::bands();
        let global = match self {
            Self::SafeStart => GlobalParams {
                input_gain_db: GainDb::new_const(0.0),
                output_gain_db: GainDb::new_const(-18.0),
                depth: UnitInterval::new_const(0.5),
                time: UnitInterval::new_const(0.5),
                upward: UnitInterval::new_const(1.0),
                downward: UnitInterval::new_const(1.0),
                crossovers: CrossoverPair::new_const(120.0, 2500.0),
            },
            Self::Default => GlobalParams {
                input_gain_db: GainDb::new_const(0.0),
                output_gain_db: GainDb::new_const(0.0),
                depth: UnitInterval::new_const(1.0),
                time: UnitInterval::new_const(0.5),
                upward: UnitInterval::new_const(1.0),
                downward: UnitInterval::new_const(1.0),
                crossovers: CrossoverPair::new_const(120.0, 2500.0),
            },
        };
        OttParams { global, bands }
    }

    /// Returns the preset's `--preset` CLI value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SafeStart => "safe-start",
            Self::Default => "default",
        }
    }
}

impl FromStr for Preset {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "safe-start" => Ok(Self::SafeStart),
            "default" => Ok(Self::Default),
            _ => Err(()),
        }
    }
}

/// Result of interpreting CLI arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum CliOutcome {
    /// Parameters to start `oxtt` with.
    Run(OttParams),
    /// The `--help` text to print before exiting.
    Help(String),
    /// The `--version` text to print before exiting.
    Version(String),
}

/// Error interpreting CLI arguments.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CliError {
    /// An option name that isn't recognized.
    #[error("unknown option: {0}")]
    UnknownOption(String),
    /// An option that requires a value but got none.
    #[error("missing value for option: {0}")]
    MissingValue(String),
    /// An option's value failed to parse or fell outside its allowed range.
    #[error("invalid value for {option}: {value}")]
    InvalidValue {
        /// The option name.
        option: String,
        /// The value that failed to parse.
        value: String,
    },
    /// The fully parsed parameters failed validation.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

fn split_inline_value(arg: &str) -> (&str, Option<&str>) {
    arg.find('=')
        .map_or((arg, None), |pos| (&arg[..pos], Some(&arg[pos + 1..])))
}

fn take_value(
    name: &str,
    inline: Option<&str>,
    iter: &mut impl Iterator<Item = String>,
) -> Result<String, CliError> {
    if let Some(v) = inline {
        return Ok(v.to_owned());
    }
    iter.next()
        .ok_or_else(|| CliError::MissingValue(name.to_owned()))
}

fn parse_f32(option: &str, value: &str) -> Result<f32, CliError> {
    value
        .trim()
        .parse::<f32>()
        .map_err(|_| CliError::InvalidValue {
            option: option.to_owned(),
            value: value.to_owned(),
        })
}

/// Normalizes a `0..100` percentage value to `0.0..1.0`. Rejects out-of-range or non-finite values before startup.
fn parse_percent(option: &str, value: &str) -> Result<f32, CliError> {
    let raw = parse_f32(option, value)?;
    if !raw.is_finite() || !(0.0..=100.0).contains(&raw) {
        return Err(CliError::InvalidValue {
            option: option.to_owned(),
            value: value.to_owned(),
        });
    }
    Ok(raw / 100.0)
}

/// Contents of the `--help` output (docs/contracts.md §1).
#[must_use]
pub fn help_text() -> String {
    format!(
        "oxtt {version} - a 3-band upward/downward multiband compressor (OTT-style)\n\n\
         USAGE:\n    oxtt [OPTIONS]\n\n\
         OPTIONS:\n\
         \x20   --preset <safe-start|default>    startup preset [default: safe-start]\n\
         \x20   --input-gain <dB>                pre-split gain, range -24..24 [default: 0]\n\
         \x20   --output-gain <dB>                post-sum gain, range -24..24 [default: -18]\n\
         \x20   --depth <%>                       dry/wet, range 0..100 [default: 50]\n\
         \x20   --time <%>                        attack/release multiplier, range 0..100 [default: 50]\n\
         \x20   --upward <%>                      upward amount multiplier, range 0..100 [default: 100]\n\
         \x20   --downward <%>                    downward amount multiplier, range 0..100 [default: 100]\n\
         \x20   --low-crossover <Hz>               low/mid split, range 40..2000 [default: 120]\n\
         \x20   --high-crossover <Hz>              mid/high split, range 400..16000 [default: 2500]\n\
         \x20   --help                            show this help and exit\n\
         \x20   --version                         show version and exit\n\n\
         NOTE: `default` preset is intentionally strong and can exceed 0 dBFS.\n\
         Start with `safe-start` and a low monitor level.\n",
        version = env!("CARGO_PKG_VERSION")
    )
}

/// Contents of the `--version` output.
#[must_use]
pub fn version_text() -> String {
    format!("oxtt {}\n", env!("CARGO_PKG_VERSION"))
}

/// Interprets command-line arguments. Do not include `argv[0]`.
///
/// Most fields are validated the moment their flag is parsed. The crossover
/// pair is the one exception: `--low-crossover`/`--high-crossover` are
/// independent flags that can appear in either order, and their octave-
/// separation invariant can only be checked once both are known. Staging
/// them as plain locals (rather than validating each flag against whatever
/// the other currently holds) keeps the result independent of argument order.
///
/// # Errors
///
/// Returns `CliError` for an unknown option, a missing or unparsable value,
/// or parameters that fail validation.
pub fn parse_args<I, S>(args: I) -> Result<CliOutcome, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut preset = Preset::default();
    let mut params = preset.params();
    let mut low_hz = params.global.crossovers.low_hz();
    let mut high_hz = params.global.crossovers.high_hz();
    let mut iter = args.into_iter().map(|s| s.as_ref().to_owned());

    while let Some(arg) = iter.next() {
        let (name, inline) = split_inline_value(&arg);
        let name = name.to_owned();
        match name.as_str() {
            "--help" => return Ok(CliOutcome::Help(help_text())),
            "--version" => return Ok(CliOutcome::Version(version_text())),
            "--preset" => {
                let value = take_value(&name, inline, &mut iter)?;
                preset = value.parse().map_err(|()| CliError::InvalidValue {
                    option: name.clone(),
                    value: value.clone(),
                })?;
                params = preset.params();
                low_hz = params.global.crossovers.low_hz();
                high_hz = params.global.crossovers.high_hz();
            }
            "--input-gain" => {
                let value = take_value(&name, inline, &mut iter)?;
                let db = parse_f32(&name, &value)?;
                params.global.input_gain_db = GainDb::new("input_gain_db", db)?;
            }
            "--output-gain" => {
                let value = take_value(&name, inline, &mut iter)?;
                let db = parse_f32(&name, &value)?;
                params.global.output_gain_db = GainDb::new("output_gain_db", db)?;
            }
            "--depth" => {
                let value = take_value(&name, inline, &mut iter)?;
                let raw = parse_percent(&name, &value)?;
                params.global.depth = UnitInterval::new("depth", raw)?;
            }
            "--time" => {
                let value = take_value(&name, inline, &mut iter)?;
                let raw = parse_percent(&name, &value)?;
                params.global.time = UnitInterval::new("time", raw)?;
            }
            "--upward" => {
                let value = take_value(&name, inline, &mut iter)?;
                let raw = parse_percent(&name, &value)?;
                params.global.upward = UnitInterval::new("upward", raw)?;
            }
            "--downward" => {
                let value = take_value(&name, inline, &mut iter)?;
                let raw = parse_percent(&name, &value)?;
                params.global.downward = UnitInterval::new("downward", raw)?;
            }
            "--low-crossover" => {
                let value = take_value(&name, inline, &mut iter)?;
                low_hz = parse_f32(&name, &value)?;
            }
            "--high-crossover" => {
                let value = take_value(&name, inline, &mut iter)?;
                high_hz = parse_f32(&name, &value)?;
            }
            _ => return Err(CliError::UnknownOption(arg)),
        }
    }

    params.global.crossovers = CrossoverPair::new(low_hz, high_hz)?;
    Ok(CliOutcome::Run(params))
}

#[cfg(test)]
// These tests unwrap known-valid results and compare exact literal values,
// so unwrap/panic/float_cmp noise here is expected rather than a real risk.
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn safe_start_and_default_params_are_valid() {
        Preset::SafeStart.params().validate(48_000.0).unwrap();
        Preset::Default.params().validate(48_000.0).unwrap();
    }

    #[test]
    fn presets_share_band_values() {
        assert_eq!(
            Preset::SafeStart.params().bands,
            Preset::Default.params().bands
        );
    }

    #[test]
    fn gain_db_rejects_nan_and_infinite() {
        assert!(matches!(
            GainDb::new("input_gain_db", f32::NAN),
            Err(ConfigError::NotFinite { .. })
        ));
        assert!(matches!(
            GainDb::new("output_gain_db", f32::INFINITY),
            Err(ConfigError::NotFinite { .. })
        ));
    }

    #[test]
    fn gain_db_rejects_out_of_range() {
        assert!(matches!(
            GainDb::new("input_gain_db", 100.0),
            Err(ConfigError::OutOfRange { .. })
        ));
    }

    #[test]
    fn unit_interval_rejects_out_of_range() {
        assert!(matches!(
            UnitInterval::new("depth", 1.5),
            Err(ConfigError::OutOfRange { .. })
        ));
    }

    #[test]
    #[should_panic(expected = "ThresholdRange lower_db must be less than upper_db")]
    fn threshold_range_new_const_rejects_inverted_thresholds() {
        ThresholdRange::new_const(-10.0, -20.0);
    }

    #[test]
    fn crossover_pair_rejects_less_than_one_octave_apart() {
        assert!(matches!(
            CrossoverPair::new(1000.0, 1500.0),
            Err(ConfigError::CrossoverOctave { .. })
        ));
    }

    #[test]
    fn rejects_crossover_above_nyquist_ratio() {
        let mut params = Preset::SafeStart.params();
        params.global.crossovers = CrossoverPair::new(params.global.crossovers.low_hz(), 8000.0)
            .unwrap();
        // At 44.1kHz, 0.45*44100 = 19845Hz, so 8kHz is allowed, but confirm it
        // violates the Nyquist constraint near an 8kHz sample-rate boundary.
        assert!(params.validate(16_000.0).is_err());
    }

    #[test]
    fn rejects_sample_rate_out_of_range() {
        assert!(validate_sample_rate(1_000.0).is_err());
        assert!(validate_sample_rate(500_000.0).is_err());
        assert!(validate_sample_rate(f32::NAN).is_err());
        assert!(validate_sample_rate(48_000.0).is_ok());
    }

    #[test]
    fn parses_long_options_with_space_and_equals() {
        let outcome = parse_args([
            "--preset",
            "default",
            "--depth=75",
            "--low-crossover",
            "100",
        ])
        .unwrap();
        match outcome {
            CliOutcome::Run(params) => {
                assert_eq!(params.global.depth.get(), 0.75);
                assert_eq!(params.global.crossovers.low_hz(), 100.0);
                assert_eq!(params.global.output_gain_db.get(), 0.0); // from `default` preset
            }
            _ => panic!("expected Run outcome"),
        }
    }

    #[test]
    fn crossover_flags_are_order_independent() {
        // low_hz=1900/high_hz=2500 (the SafeStart default) would violate the
        // octave constraint, but high_hz=8000 fixes it. Staging both flags
        // before validating means this succeeds regardless of flag order.
        let a = parse_args(["--low-crossover", "1900", "--high-crossover", "8000"]).unwrap();
        let b = parse_args(["--high-crossover", "8000", "--low-crossover", "1900"]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert!(matches!(
            parse_args(["--help"]).unwrap(),
            CliOutcome::Help(_)
        ));
        assert!(matches!(
            parse_args(["--version"]).unwrap(),
            CliOutcome::Version(_)
        ));
    }

    #[test]
    fn rejects_unknown_option() {
        assert!(matches!(
            parse_args(["--bogus"]),
            Err(CliError::UnknownOption(_))
        ));
    }

    #[test]
    fn rejects_missing_value() {
        assert!(matches!(
            parse_args(["--depth"]),
            Err(CliError::MissingValue(_))
        ));
    }

    #[test]
    fn rejects_invalid_numeric_value() {
        assert!(matches!(
            parse_args(["--depth", "not-a-number"]),
            Err(CliError::InvalidValue { .. })
        ));
    }

    #[test]
    fn individual_options_override_preset() {
        let outcome = parse_args(["--preset", "default", "--output-gain", "-6"]).unwrap();
        match outcome {
            CliOutcome::Run(params) => assert_eq!(params.global.output_gain_db.get(), -6.0),
            _ => panic!("expected Run outcome"),
        }
    }
}
