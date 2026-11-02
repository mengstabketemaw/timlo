//! Shared logic for the day planner, used by the Linux daemon and (later) the
//! Tauri app on Linux and Android.
//!
//! - [`model`]: templates, blocks, days and statuses.
//! - [`store`]: SQLite storage plus the template → day rules.
//! - [`now`]: "what's now / what's next".
//! - [`reminders`]: which reminders are due in a minute window.
//! - [`settings`]: reminder timing, quiet hours.
//! - [`plan_file`]: the hand-edited TOML plan file (milestone 2).

pub mod model;
pub mod now;
pub mod plan_file;
pub mod reminders;
pub mod settings;
pub mod store;
pub mod time;

pub use model::{BlockInput, Day, DayBlock, Status, Template, TemplateBlock};
pub use settings::Settings;
pub use store::Store;
