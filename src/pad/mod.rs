//! Everything about a physical pad: the state types, the report parsers, the output writer, the reader threads.
//!
//! Layering, so later sections can plug in without touching the reader:
//! - [`state`]: [`PadState`], the one parsed shape used everywhere (tester UI, emulation, mapping);
//! - [`parse`]: input report bytes to `PadState`, and the calibration maths;
//! - [`output`]: [`Output`] (what we want the pad to do) and the report builders, with the Bluetooth CRC32;
//! - [`hub`]: a reader thread per pad, hot-plug scanning and one writer with a dirty flag;
//! - [`demo`]: a scripted fake pad that goes through the same pipeline, with no hardware;
//! - [`analysis`]: drift and circularity verdicts on sample streams.
//!
//! Hooks for the next sections: section B registers a [`hub::PadListener`] (called on the reader thread for every
//! parsed report, no sleeps) to feed the virtual Xbox pad, and writes game rumble through [`hub::Hub::set_output`].
//! Section C inserts its mapping layer between the listener and the virtual pad.

pub mod analysis;
pub mod demo;
pub mod hub;
pub mod output;
pub mod parse;
pub mod state;
#[cfg(target_os = "linux")]
pub mod linux_led;

pub use state::*;
