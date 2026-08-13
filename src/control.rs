//! The physical control surface: six potentiometers and a latching bypass switch.
//!
//! Split into three layers, so that the middle one — the only one with any
//! behaviour worth testing — is shared by every platform:
//!
//! | Layer | Responsibility | Platform |
//! |---|---|---|
//! | A: raw read | produce a [`RawControls`] value | platform-specific (Raspberry Pi SPI/GPIO, Bela analog/digital inputs) |
//! | B: mapping | jitter filtering, deadband, and turning raw controls into a complete [`ControlSnapshot`](crate::params::ControlSnapshot) | shared |
//! | C: transport | control thread plus a `triple_buffer` handoff into the audio callback | Raspberry Pi only |
//!
//! Layer B ([`ControlMapping`]) is pure: no I/O, no threads, no clock, no
//! allocation, and no panic. That is the entire reason for the split. On a
//! Raspberry Pi the audio callback cannot read SPI itself, so layer C exists
//! to move a finished `ControlSnapshot` across the thread boundary without a
//! lock (docs/contracts.md §6). On a Bela the controls are read inside its own
//! real-time callback, which then drives layer B *directly* and skips layer C
//! entirely — which is only possible because layer B obeys the same
//! prohibitions as the audio callback in docs/contracts.md §6 (ADR 0011).
//! Layer C is therefore compiled only under the `jack-host` feature.
//!
//! The seam between layer A and layer B is the [`RawControls`] *value*, not a
//! trait. [`ControlSource`] is the Raspberry Pi's shape for producing one —
//! hardware owned by the implementation, polled from a thread, able to fail —
//! and the Bela host matches none of those, so it builds a `RawControls`
//! directly (`crate::bela_host`). The one `ControlSource` implementation that
//! talks to hardware is `PiControls` (module `pi`), compiled only under the
//! `pi-controls` feature — deliberately not linked here, because `rppal` is
//! Linux-only and this module has to document itself on any platform.

mod mapping;
#[cfg(feature = "pi-controls")]
mod pi;
mod raw;
#[cfg(feature = "jack-host")]
mod thread;

pub use mapping::ControlMapping;
#[cfg(feature = "pi-controls")]
pub use pi::{PiControlError, PiControls};
pub use raw::{ControlSource, POT_POSITION_MAX, PotPosition, Pots, RawControls};
#[cfg(feature = "jack-host")]
pub use thread::{ControlHandle, DEFAULT_POLL_INTERVAL};
