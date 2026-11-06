//! Reviewed built-in plugin logic only. Process isolation is not an OS sandbox.
//! Installed third-party plugins must not be routed through this module.

pub mod broker;
pub mod protocol;
pub mod worker;

pub use broker::Broker;
pub use protocol::{
    Action, Error, Project, Row, Screen, ScreenAction, ScreenState, Status, StorageSnapshot,
    TaskType,
};
