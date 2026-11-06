//! Voice tasks: a spoken task becomes a Google Tasks entry, and an event in
//! Google Calendar when it has a clock time.

pub mod parse;
pub mod settings;
pub mod when;

pub use parse::{Draft, List};
pub use settings::TaskSettings;
pub use when::Said;
