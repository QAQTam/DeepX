//! deepx-session — unified session manager singleton.
//!
//! Follows the same pattern as deepx-tools::ToolManager.

pub mod manager;
pub mod session_meta;
pub mod store;
mod migrate;
pub use session_meta::SessionMeta;
pub use manager::SessionManager;
