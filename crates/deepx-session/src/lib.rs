//! deepx-session — unified session manager singleton.
//!
//! Follows the same pattern as deepx-tools::ToolManager.

pub mod manager;
mod migrate;
pub mod mirror;
pub mod session_meta;
pub mod store;
pub use manager::SessionManager;
pub use mirror::{MirrorManifest, MirrorSnapshot};
pub use session_meta::SessionMeta;
