//! Carrying oxtt's control surface into the audio callback.
//!
//! The surface itself — six potentiometers, a latching bypass switch, and the
//! conditioning that turns a hardware read into usable travel — belongs to
//! [`effectkit_controls`] and knows nothing about this effect. Which pot does
//! what is [`oxtt_controls`]. What is left here is layer C:
//!
//! | Layer | Responsibility | Where |
//! |---|---|---|
//! | A: raw read | produce a `RawControls` value | `effectkit_controls::gem`, or `effectkit-controls-pi` |
//! | B1: conditioning | jitter filter, deadband, debounce, normalisation | `effectkit_controls::SixPotBypassConditioner` |
//! | **B2: assignment** | **what each pot *does*** | `oxtt_controls::assign` |
//! | **C: transport** | **control thread plus a `triple_buffer` handoff into the audio callback** | [`ControlHandle`], here, JACK only |
//!
//! **B1 and B2 are separate crates because only B1 generalises.** A second
//! effect on the same six pots and the same switch wants the identical filter,
//! deadband and debounce, and a different answer to what the knobs mean.
//! Splitting them leaves neither half owning the base parameters, so whoever
//! joins the two owns them: the control thread here under JACK,
//! `OttApplication` under Bela.
//!
//! Layer C exists because on a Raspberry Pi the audio callback cannot read SPI
//! itself, so a finished `OttProcessorUpdate` has to cross a thread boundary
//! without a lock (docs/oxtt/contracts.md §6). On a Bela the controls are read
//! inside the audio callback, which then runs B1 and B2 *directly* and skips
//! layer C entirely — possible only because both obey the same prohibitions as
//! the callback (ADR 0011). That is why layer C lives in this package rather
//! than a shared one: `oxtt-bela` has no use for it.
//!
//! It has not been generalised into `effectkit-controls` along with B1 because
//! it has one user. A transport layer worth sharing would take the assignment
//! as a type parameter or a closure, and there is no second implementation to
//! check that guess against — hyperglare is Bela-only, so one is not coming
//! from there either.
//!
//! The Raspberry Pi's layer A is reached through the `pi-controls` feature,
//! because its `rppal` dependency is Linux-only.

mod thread;

pub use thread::ControlHandle;
