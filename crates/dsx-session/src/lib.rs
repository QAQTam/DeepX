//! dsx-session — unified session manager singleton.
//!
//! Follows the same pattern as dsx-tools::ToolManager.

pub mod manager;
pub mod session_meta;
pub use session_meta::SessionMeta;
pub use manager::SessionManager;
