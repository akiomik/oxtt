//! Command-line arguments for the `oxtt` binary (docs/contracts.md §1).
//!
//! The startup preset and the global parameter overrides are shared with the
//! other two binaries and live in [`oxtt_args`]; what is here is the JACK
//! host's own.

use clap::Parser;

use oxtt_args::ParamsArgs;
use oxtt_dsp::params::{ConfigError, OttParams};

/// Command-line arguments for the JACK host.
#[derive(Parser, Debug, Clone)]
#[command(
    version,
    about,
    long_about = None,
    after_help = "NOTE: `default` and `riot` presets are intentionally strong and can exceed 0 dBFS.\nStart with `safe-start` and a low monitor level.",
    allow_negative_numbers = true
)]
pub struct Cli {
    /// Startup preset and global parameter overrides.
    #[command(flatten)]
    pub params: ParamsArgs,

    /// print the JACK xrun count to stderr after a normal exit
    #[arg(long)]
    pub report_xruns_on_exit: bool,

    /// drive depth/time/upward/downward from the hardware control surface
    /// (MCP3008 pots on SPI0/CE0, bypass switch on GPIO17)
    ///
    /// Opt-in rather than on by default even in a `pi-controls` build: the
    /// same binary has to stay runnable on a Pi with no breadboard attached,
    /// which is how the audio verification scripts under `scripts/` run it.
    /// The flag does not exist at all without the feature, so a build that
    /// cannot read the hardware cannot be asked to.
    #[cfg(feature = "pi-controls")]
    #[arg(long)]
    pub controls: bool,
}

impl TryFrom<Cli> for OttParams {
    type Error = ConfigError;

    fn try_from(cli: Cli) -> Result<Self, ConfigError> {
        cli.params.try_into()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use oxtt_dsp::params::Preset;

    use super::*;

    #[test]
    fn xrun_report_is_opt_in() {
        assert!(!Cli::parse_from(["oxtt"]).report_xruns_on_exit);
        assert!(Cli::parse_from(["oxtt", "--report-xruns-on-exit"]).report_xruns_on_exit);
    }

    /// The control surface must stay off unless it is asked for, so that a
    /// `pi-controls` build with no breadboard attached still starts (see the
    /// flag's own documentation).
    #[cfg(feature = "pi-controls")]
    #[test]
    fn the_control_surface_is_opt_in() {
        assert!(!Cli::parse_from(["oxtt"]).controls);
        assert!(Cli::parse_from(["oxtt", "--controls"]).controls);
    }

    /// Without the feature the flag is not merely off, it does not exist —
    /// asking for it is an argument error rather than a silent no-op.
    #[cfg(not(feature = "pi-controls"))]
    #[test]
    fn there_is_no_control_surface_flag_without_the_feature() {
        assert!(
            Cli::try_parse_from(["oxtt", "--controls"]).is_err(),
            "--controls must not parse in a build that cannot read the hardware"
        );
    }

    /// The shared arguments reach `OttParams` through this parser too, not
    /// only through their own tests in `oxtt-args`.
    #[test]
    fn the_shared_arguments_flatten_into_this_parser() {
        let cli = Cli::parse_from(["oxtt", "--preset", "riot"]);
        assert_eq!(OttParams::try_from(cli).unwrap(), Preset::Riot.params());
    }
}
