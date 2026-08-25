//! oxtt's end of the physical control surface.
//!
//! The surface itself — six potentiometers, a latching bypass switch, and the
//! conditioning that turns a hardware read into usable travel — belongs to
//! [`effectkit_controls`] and knows nothing about this effect. What is left
//! here is the part that does:
//!
//! | Layer | Responsibility | Where |
//! |---|---|---|
//! | A: raw read | produce a `RawControls` value | `effectkit_controls::gem`, or [`PiControls`] (module `pi`) |
//! | B1: conditioning | jitter filter, deadband, debounce, normalisation | `effectkit_controls::SixPotBypassConditioner` |
//! | **B2: assignment** | **what each pot *does*** | [`assign`], here |
//! | **C: transport** | **control thread plus a `triple_buffer` handoff into the audio callback** | [`ControlHandle`], here, JACK only |
//!
//! **B1 and B2 are separate because only B1 generalises.** A second effect on
//! the same six pots and the same switch wants the identical filter, deadband
//! and debounce, and a different answer to what the knobs mean. Splitting them
//! leaves neither half owning the base parameters, so whoever joins the two
//! owns them: the control thread under JACK, `OttApplication` under Bela.
//!
//! Layer C exists because on a Raspberry Pi the audio callback cannot read SPI
//! itself, so a finished `OttProcessorUpdate` has to cross a thread boundary
//! without a lock (docs/contracts.md §6). On a Bela the controls are read
//! inside the audio callback, which then runs B1 and B2 *directly* and skips
//! layer C entirely — possible only because both obey the same prohibitions as
//! the callback (ADR 0011). Layer C is therefore compiled only under the
//! `jack-host` feature.
//!
//! It has not been generalised into `effectkit-controls` along with B1 because
//! it has one user. A transport layer worth sharing would take the assignment
//! as a type parameter or a closure, and there is no second implementation to
//! check that guess against — hyperglare is Bela-only, so one is not coming
//! from there either.
//!
//! [`PiControls`] (module `pi`) is compiled only under the `pi-controls`
//! feature, because `rppal` is Linux-only.

mod assign;
#[cfg(feature = "pi-controls")]
mod pi;
#[cfg(feature = "jack-host")]
mod thread;

pub use assign::assign;
#[cfg(feature = "pi-controls")]
pub use pi::{PiControlError, PiControls};
#[cfg(feature = "jack-host")]
pub use thread::ControlHandle;
