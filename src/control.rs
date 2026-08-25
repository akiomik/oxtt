//! The physical control surface: six potentiometers and a latching bypass switch.
//!
//! Split into layers, so that the ones with behaviour worth testing are shared
//! by every platform:
//!
//! | Layer | Responsibility | Scope |
//! |---|---|---|
//! | A: raw read | produce a [`RawControls`] value | platform-specific (Raspberry Pi SPI/GPIO, Bela analog/digital inputs) |
//! | B1: conditioning | jitter filter, deadband, switch debounce, normalisation onto [`PotTravel`] → [`ConditionedControls`] | shared by every effect |
//! | B2: assignment | what each pot *does*: [`ConditionedControls`] → [`OttProcessorUpdate`](crate::params::OttProcessorUpdate) | this effect only |
//! | C: transport | control thread plus a `triple_buffer` handoff into the audio callback | Raspberry Pi only |
//!
//! **B1 and B2 are separate because only B1 generalises.** A second effect on
//! the same six pots and the same switch wants the identical filter, deadband
//! and debounce, and a different answer to what the knobs mean. Splitting them
//! leaves neither half owning the base parameters, so whoever joins the two
//! owns them: the control thread under JACK, `OttApplication` under Bela.
//!
//! [`SixPotBypassConditioner`] is pure: no I/O, no threads, no clock, no
//! allocation, and no panic. That is the entire reason for the split. On a
//! Raspberry Pi the audio callback cannot read SPI itself, so layer C exists
//! to move a finished `OttProcessorUpdate` across the thread boundary without a
//! lock (docs/contracts.md §6). On a Bela the controls are read inside its own
//! real-time callback, which then drives B1 and B2 *directly* and skips layer C
//! entirely — which is only possible because both obey the same prohibitions
//! as the audio callback in docs/contracts.md §6 (ADR 0011). Layer C is
//! therefore compiled only under the `jack-host` feature.
//!
//! The seam between layer A and layer B is the [`RawControls`] *value*, not a
//! trait. [`ControlSource`] is the Raspberry Pi's shape for producing one —
//! hardware owned by the implementation, polled from a thread, able to fail —
//! and the Bela host matches none of those, so it builds a `RawControls`
//! directly (`crate::bela_host`). The one `ControlSource` implementation that
//! talks to hardware is `PiControls` (module `pi`), compiled only under the
//! `pi-controls` feature — deliberately not linked here, because `rppal` is
//! Linux-only and this module has to document itself on any platform.

mod assign;
pub(crate) mod conditioning;
#[cfg(feature = "pi-controls")]
mod pi;
mod raw;
pub mod surfaces;
#[cfg(feature = "jack-host")]
mod thread;

pub use assign::assign;
pub use conditioning::{ConditionedControls, PotTravel, SixPotBypassConditioner};
pub use conditioning::{
    ConditioningConfig, DeadbandCounts, DebounceReads, FilterCoefficient, PollHz,
};
#[cfg(feature = "pi-controls")]
pub use pi::{PiControlError, PiControls};
pub use raw::{ControlSource, POT_POSITION_MAX, PotPosition, Pots, RawControls};
#[cfg(feature = "jack-host")]
pub use thread::ControlHandle;
