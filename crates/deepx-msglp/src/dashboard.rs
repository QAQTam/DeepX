use crate::agent::AgentState;
use deepx_proto::DocInfo;
use deepx_proto::TaskInfo;

pub fn build_documents(_agent: &AgentState) -> Vec<DocInfo> {
    let files_read = deepx_tools::bridge::files_read();
    let mut docs: Vec<DocInfo> = files_read
        .iter()
        .map(|path| {
            let tag = String::from("doc");
            DocInfo {
                tag,
                path: path.clone(),
                turns_since_read: 1,
                is_stale: false,
            }
        })
        .collect();
    docs.truncate(20);
    docs
}

pub fn build_recent_edits(_agent: &AgentState) -> Vec<String> {
    let files = deepx_tools::bridge::files_written();
    files.iter().take(10).map(|f| format!("edit: {}", f)).collect()
}

pub fn build_tasks(_agent: &AgentState) -> Vec<TaskInfo> {
    Vec::new()
}
