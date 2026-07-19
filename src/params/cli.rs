//! Interprets CLI arguments into `OttParams` (docs/contracts.md §1).

use thiserror::Error;

use super::error::ConfigError;
use super::model::OttParams;
use super::preset::Preset;
use super::value::{CrossoverPair, GainDb, UnitInterval};

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
#[allow(clippy::unwrap_used, clippy::panic, clippy::float_cmp)]
mod tests {
    use super::*;

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
