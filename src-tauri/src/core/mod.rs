pub mod autostart;
pub mod backup;
pub mod handle;
pub mod hotkey;
pub mod logger;
pub mod manager;
mod notification;
mod owner_identity;
mod runtime_bundle;
pub mod service;
pub mod sysopt;
pub mod timer;
pub mod tray;
pub mod updater;
pub mod validate;

pub use self::{manager::CoreManager, timer::Timer, updater::SilentUpdater};
