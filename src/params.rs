//! Interprets CLI values, and handles parameter validation, normalization, and presets.
//!
//! Ranges and invariants follow `docs/contracts.md` §1.

use std::str::FromStr;

use thiserror::Error;

/// Lower bound of the allowed sample rate range (docs/contracts.md §1).
pub const MIN_SAMPLE_RATE_HZ: f32 = 8_000.0;
/// Upper bound of the allowed sample rate range (docs/contracts.md §1).
pub const MAX_SAMPLE_RATE_HZ: f32 = 384_000.0;

/// Upper-bound coefficient on the Nyquist side that crossover frequencies must respect (docs/contracts.md §1).
pub const CROSSOVER_NYQUIST_RATIO: f32 = 0.45;

/// Global parameters shared across all bands (docs/contracts.md §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalParams {
    /// Pre-split gain in dB, range `-24.0..=24.0`.
    pub input_gain_db: f32,
    /// Post-sum gain in dB, range `-24.0..=24.0`.
    pub output_gain_db: f32,
    /// Dry/wet mix, range `0.0..=1.0`.
    pub depth: f32,
    /// Attack/release time multiplier, range `0.0..=1.0`.
    pub time: f32,
    /// Upward compression amount multiplier, range `0.0..=1.0`.
    pub upward: f32,
    /// Downward compression amount multiplier, range `0.0..=1.0`.
    pub downward: f32,
    /// Low/mid crossover frequency in Hz, range `40.0..=2000.0`.
    pub low_crossover_hz: f32,
    /// Mid/high crossover frequency in Hz, range `400.0..=16000.0`.
    pub high_crossover_hz: f32,
}

/// Per-band parameters (docs/contracts.md §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandParams {
    /// Downward-compression threshold in dB, range `-80.0..=0.0`.
    pub lower_threshold_db: f32,
    /// Upward-compression threshold in dB, range `-80.0..=0.0`, must exceed `lower_threshold_db`.
    pub upper_threshold_db: f32,
    /// Upward compression amount, range `0.0..=1.0`.
    pub up_amount: f32,
    /// Downward compression amount, range `0.0..=1.0`.
    pub down_amount: f32,
    /// Makeup gain in dB, range `-40.0..=40.0`.
    pub makeup_gain_db: f32,
    /// Attack time in ms at `time = 0.5`, must be positive.
    pub base_attack_ms: f32,
    /// Release time in ms at `time = 0.5`, must be positive.
    pub base_release_ms: f32,
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
    /// A band's `lower_threshold_db` was not less than its `upper_threshold_db`.
    #[error(
        "band {band}: lower_threshold_db ({lower}) must be less than upper_threshold_db ({upper})"
    )]
    ThresholdOrder {
        /// Index of the offending band.
        band: usize,
        /// The band's `lower_threshold_db`.
        lower: f32,
        /// The band's `upper_threshold_db`.
        upper: f32,
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

fn check_positive(field: &'static str, value: f32) -> Result<(), ConfigError> {
    check_finite(field, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(ConfigError::OutOfRange {
            field,
            min: f32::EPSILON,
            max: f32::INFINITY,
            value,
        })
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

impl BandParams {
    fn validate(&self, band: usize) -> Result<(), ConfigError> {
        check_range("lower_threshold_db", self.lower_threshold_db, -80.0, 0.0)?;
        check_range("upper_threshold_db", self.upper_threshold_db, -80.0, 0.0)?;
        if self.lower_threshold_db >= self.upper_threshold_db {
            return Err(ConfigError::ThresholdOrder {
                band,
                lower: self.lower_threshold_db,
                upper: self.upper_threshold_db,
            });
        }
        check_range("up_amount", self.up_amount, 0.0, 1.0)?;
        check_range("down_amount", self.down_amount, 0.0, 1.0)?;
        check_range("makeup_gain_db", self.makeup_gain_db, -40.0, 40.0)?;
        check_positive("base_attack_ms", self.base_attack_ms)?;
        check_positive("base_release_ms", self.base_release_ms)?;
        Ok(())
    }
}

impl OttParams {
    /// Validates the sample-rate-independent ranges and invariants.
    ///
    /// Can be called before the sample rate that JACK will report is known,
    /// e.g. at CLI startup.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if any field is out of range, non-finite, or
    /// violates a cross-field invariant (docs/contracts.md §1).
    pub fn validate_ranges(&self) -> Result<(), ConfigError> {
        let g = &self.global;
        check_range("input_gain_db", g.input_gain_db, -24.0, 24.0)?;
        check_range("output_gain_db", g.output_gain_db, -24.0, 24.0)?;
        check_range("depth", g.depth, 0.0, 1.0)?;
        check_range("time", g.time, 0.0, 1.0)?;
        check_range("upward", g.upward, 0.0, 1.0)?;
        check_range("downward", g.downward, 0.0, 1.0)?;
        check_range("low_crossover_hz", g.low_crossover_hz, 40.0, 2000.0)?;
        check_range("high_crossover_hz", g.high_crossover_hz, 400.0, 16000.0)?;

        if g.high_crossover_hz < 2.0 * g.low_crossover_hz {
            return Err(ConfigError::CrossoverOctave {
                low_hz: g.low_crossover_hz,
                high_hz: g.high_crossover_hz,
            });
        }

        for (i, band) in self.bands.iter().enumerate() {
            band.validate(i)?;
        }

        Ok(())
    }

    /// Full validation including the sample-rate-dependent Nyquist constraint (docs/contracts.md §1).
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `sample_rate` is invalid, [`Self::validate_ranges`]
    /// fails, or a crossover frequency exceeds the Nyquist-relative limit.
    pub fn validate(&self, sample_rate: f32) -> Result<(), ConfigError> {
        validate_sample_rate(sample_rate)?;
        self.validate_ranges()?;

        let nyquist_limit = CROSSOVER_NYQUIST_RATIO * sample_rate;
        let g = &self.global;
        if g.low_crossover_hz > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "low_crossover_hz",
                value: g.low_crossover_hz,
                max: nyquist_limit,
            });
        }
        if g.high_crossover_hz > nyquist_limit {
            return Err(ConfigError::CrossoverNyquist {
                field: "high_crossover_hz",
                value: g.high_crossover_hz,
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
        lower_threshold_db: -35.0,
        upper_threshold_db: -28.0,
        up_amount: 0.800,
        down_amount: 0.900,
        makeup_gain_db: 16.3,
        base_attack_ms: 2.8,
        base_release_ms: 40.0,
    };
    const MID_BAND: BandParams = BandParams {
        lower_threshold_db: -36.0,
        upper_threshold_db: -25.0,
        up_amount: 0.800,
        down_amount: 0.857,
        makeup_gain_db: 11.7,
        base_attack_ms: 1.4,
        base_release_ms: 28.0,
    };
    const HIGH_BAND: BandParams = BandParams {
        lower_threshold_db: -35.0,
        upper_threshold_db: -30.0,
        up_amount: 0.800,
        down_amount: 1.000,
        makeup_gain_db: 16.3,
        base_attack_ms: 0.7,
        base_release_ms: 15.0,
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
                input_gain_db: 0.0,
                output_gain_db: -18.0,
                depth: 0.5,
                time: 0.5,
                upward: 1.0,
                downward: 1.0,
                low_crossover_hz: 120.0,
                high_crossover_hz: 2500.0,
            },
            Self::Default => GlobalParams {
                input_gain_db: 0.0,
                output_gain_db: 0.0,
                depth: 1.0,
                time: 0.5,
                upward: 1.0,
                downward: 1.0,
                low_crossover_hz: 120.0,
                high_crossover_hz: 2500.0,
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
            }
            "--input-gain" => {
                let value = take_value(&name, inline, &mut iter)?;
                params.global.input_gain_db = parse_f32(&name, &value)?;
            }
            "--output-gain" => {
                let value = take_value(&name, inline, &mut iter)?;
                params.global.output_gain_db = parse_f32(&name, &value)?;
            }
            "--depth" => {
                let value = take_value(&name, inline, &mut iter)?;
                params.global.depth = parse_percent(&name, &value)?;
            }
            "--time" => {
                let value = take_value(&name, inline, &mut iter)?;
                params.global.time = parse_percent(&name, &value)?;
            }
            "--upward" => {
                let value = take_value(&name, inline, &mut iter)?;
                params.global.upward = parse_percent(&name, &value)?;
            }
            "--downward" => {
                let value = take_value(&name, inline, &mut iter)?;
                params.global.downward = parse_percent(&name, &value)?;
            }
            "--low-crossover" => {
                let value = take_value(&name, inline, &mut iter)?;
                params.global.low_crossover_hz = parse_f32(&name, &value)?;
            }
            "--high-crossover" => {
                let value = take_value(&name, inline, &mut iter)?;
                params.global.high_crossover_hz = parse_f32(&name, &value)?;
            }
            _ => return Err(CliError::UnknownOption(arg)),
        }
    }

    params.validate_ranges()?;
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
    fn rejects_nan_and_infinite() {
        let mut params = Preset::SafeStart.params();
        params.global.input_gain_db = f32::NAN;
        assert!(matches!(
            params.validate_ranges(),
            Err(ConfigError::NotFinite { .. })
        ));

        let mut params = Preset::SafeStart.params();
        params.global.output_gain_db = f32::INFINITY;
        assert!(matches!(
            params.validate_ranges(),
            Err(ConfigError::NotFinite { .. })
        ));
    }

    #[test]
    fn rejects_out_of_range_gain() {
        let mut params = Preset::SafeStart.params();
        params.global.input_gain_db = 100.0;
        assert!(matches!(
            params.validate_ranges(),
            Err(ConfigError::OutOfRange { .. })
        ));
    }

    #[test]
    fn rejects_inverted_thresholds() {
        let mut params = Preset::SafeStart.params();
        params.bands[BAND_LOW].lower_threshold_db = -10.0;
        params.bands[BAND_LOW].upper_threshold_db = -20.0;
        assert!(matches!(
            params.validate_ranges(),
            Err(ConfigError::ThresholdOrder { .. })
        ));
    }

    #[test]
    fn rejects_crossover_less_than_one_octave_apart() {
        let mut params = Preset::SafeStart.params();
        params.global.low_crossover_hz = 1000.0;
        params.global.high_crossover_hz = 1500.0;
        assert!(matches!(
            params.validate_ranges(),
            Err(ConfigError::CrossoverOctave { .. })
        ));
    }

    #[test]
    fn rejects_crossover_above_nyquist_ratio() {
        let params = OttParams {
            global: GlobalParams {
                high_crossover_hz: 8000.0,
                ..Preset::SafeStart.params().global
            },
            ..Preset::SafeStart.params()
        };
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
                assert_eq!(params.global.depth, 0.75);
                assert_eq!(params.global.low_crossover_hz, 100.0);
                assert_eq!(params.global.output_gain_db, 0.0); // from `default` preset
            }
            _ => panic!("expected Run outcome"),
        }
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
            CliOutcome::Run(params) => assert_eq!(params.global.output_gain_db, -6.0),
            _ => panic!("expected Run outcome"),
        }
    }
}
