//! Session restore: return all known sessions from index.

use dsx_types::SessionMeta;
use crate::session::persist::load_index;

pub fn find_live_sessions() -> Vec<SessionMeta> {
    let mut metas = load_index();
    metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    metas
}
