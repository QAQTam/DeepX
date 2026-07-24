//! RAG 工具：`rag_index`, `rag_search`, `memory_search`。
//!
//! - AgentState 启动时通过 `set_engine()` 注入共享 `VectorEngine`。
//! - 工具从 engine 获取 embedder/memory/search 能力。

use std::sync::{Arc, Mutex, OnceLock};

use deepx_vector::VectorEngine;

use crate::{ToolCallCtx, ToolHandler, ToolResult, ToolRisk};

// ─── 共享引擎 ──────────────────────────────��───────────────────────────────

static ENGINE: OnceLock<Arc<Mutex<VectorEngine>>> = OnceLock::new();

/// 由 AgentState 调用，注入共享引擎。
pub fn set_engine(engine: Arc<Mutex<VectorEngine>>) {
    let _ = ENGINE.set(engine);
}

macro_rules! try_or_err {
    ($expr:expr, $msg:expr) => {
        match $expr {
            Some(v) => v,
            None => return ToolResult::error($msg),
        }
    };
}

// ─── rag_index ──────────────────────────────��───────────────────────────────

pub struct RagIndexTool;

impl RagIndexTool {
    pub fn tool() -> ToolHandler {
        ToolHandler {
            key: "rag_index".into(),
            description: "索引文本到 RAG 知识库。参数: doc_id, content",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "doc_id": { "type": "string", "description": "文档唯一 ID" },
                    "content": { "type": "string", "description": "需要索引的文本内容" }
                },
                "required": ["doc_id", "content"]
            }),
            handler: Self::handle,
            risk: ToolRisk::ReadOnly,
            default_timeout: std::time::Duration::from_secs(30),
        }
    }

    fn handle(ctx: ToolCallCtx) -> ToolResult {
        let doc_id = try_or_err!(ctx.get_str("doc_id"), "缺少参数 doc_id");
        let content = try_or_err!(ctx.get_str("content"), "缺少参数 content");

        let eng = try_or_err!(ENGINE.get(), "RAG 引擎未初始化");
        let mut guard = try_or_err!(eng.lock().ok(), "引擎锁已毒化");

        if let Err(e) = guard.index_docs(&[(doc_id.to_string(), content.to_string())]) {
            return ToolResult::error(format!("索引失败: {e}"));
        }
        ToolResult::ok(format!("文档 '{}' 索引完成", doc_id))
    }
}

// ─── rag_search ─────────────────────────────────────────────────────────────

pub struct RagSearchTool;

impl RagSearchTool {
    pub fn tool() -> ToolHandler {
        ToolHandler {
            key: "rag_search".into(),
            description: "语义搜索 RAG 知识库。参数: query, top_k (默认 5)",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索查询" },
                    "top_k": { "type": "integer", "description": "返回条数，默认 5" }
                },
                "required": ["query"]
            }),
            handler: Self::handle,
            risk: ToolRisk::ReadOnly,
            default_timeout: std::time::Duration::from_secs(30),
        }
    }

    fn handle(ctx: ToolCallCtx) -> ToolResult {
        let query = try_or_err!(ctx.get_str("query"), "缺少参数 query");
        let top_k = ctx.get_u64("top_k").unwrap_or(5) as usize;

        let eng = try_or_err!(ENGINE.get(), "RAG 引擎未初始化");
        let guard = try_or_err!(eng.lock().ok(), "引擎锁已毒化");

        let results = match guard.search_docs(query, top_k) {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("搜索失败: {e}")),
        };

        if results.is_empty() {
            return ToolResult::ok("未找到相关文档。");
        }

        let out: String = results.iter().enumerate()
            .map(|(i, r)| format!("[{}] {} (score: {:.3})", i + 1, r.metadata, r.score))
            .collect::<Vec<_>>()
            .join("\n");
        ToolResult::ok(out)
    }
}

// ─── memory_search ─────────────────────────────────────────────────────────

pub struct MemorySearchTool;

impl MemorySearchTool {
    pub fn tool() -> ToolHandler {
        ToolHandler {
            key: "memory_search".into(),
            description: "搜索跨会话记忆。参数: query, limit (默认 5)",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索词" },
                    "limit": { "type": "integer", "description": "返回条数，默认 5" }
                },
                "required": ["query"]
            }),
            handler: Self::handle,
            risk: ToolRisk::ReadOnly,
            default_timeout: std::time::Duration::from_secs(30),
        }
    }

    fn handle(ctx: ToolCallCtx) -> ToolResult {
        let query = try_or_err!(ctx.get_str("query"), "缺少参数 query");
        let limit = ctx.get_u64("limit").unwrap_or(5) as usize;

        let eng = try_or_err!(ENGINE.get(), "RAG 引擎未初始化");
        let guard = try_or_err!(eng.lock().ok(), "引擎锁已毒化");

        let mut entries = guard.recall_memory_keyword(query, limit);

        if entries.len() < limit {
            if let Ok(sem) = guard.recall_memory(query, limit - entries.len()) {
                entries.extend(sem);
            }
        }

        if entries.is_empty() {
            return ToolResult::ok("未找到相关记忆。");
        }

        let out: String = entries.iter().enumerate()
            .map(|(i, e)| format!(
                "[{}] ({}..) {}",
                i + 1,
                &e.session_id[..e.session_id.len().min(12)],
                e.content
            ))
            .collect::<Vec<_>>()
            .join("\n");
        ToolResult::ok(out)
    }
}

// ─── 注册 ──────────────────────────────────────────────────────────────────

pub fn register_rag_tools(mgr: &mut crate::manager::ToolManager) {
    mgr.register(RagIndexTool::tool());
    mgr.register(RagSearchTool::tool());
    mgr.register(MemorySearchTool::tool());
}
