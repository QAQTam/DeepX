//! jaq tool — jq-compatible JSON processor powered by `jaq-core`.
//! Windows-native, no jq binary required.

use crate::{parse_arg, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

pub(super) fn exec_jaq(args: &str) -> String {
    let filter = parse_arg(args, "filter");
    let path = parse_arg(args, "path");

    if filter.is_empty() || path.is_empty() {
        return "[ERROR] jaq: filter and path required".into();
    }

    let input_str = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => return format!("[ERROR] jaq: cannot read {path}: {e}"),
    };

    let out = match run_jaq(&filter, &input_str, &path) {
        Ok(s) => s,
        Err(e) => format!("[ERROR] jaq: {e}"),
    };

    out
}

fn run_jaq(filter: &str, input: &str, path: &str) -> Result<String, String> {
    use jaq_core::{Ctx, Vars, Compiler, unwrap_valr};
    use jaq_core::load::{Arena, File, Loader};
    use jaq_json::Val;

    // Parse input JSON
    let val = jaq_json::read::parse_single(input.as_bytes())
        .map_err(|e| format!("{path}: invalid JSON: {e}"))?;

    // Set up definitions and functions
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let funs = jaq_core::funs()
        .chain(jaq_std::funs())
        .chain(jaq_json::funs());

    // Parse filter
    let program = File { code: filter, path: () };
    let loader = Loader::new(defs);
    let arena = Arena::default();
    let modules = loader.load(&arena, program)
        .map_err(|es| format!("parse error: {es:?}"))?;

    // Compile filter
    let filter_obj = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|es| format!("compile error: {es:?}"))?;

    // Execute
    let ctx = Ctx::<jaq_core::data::JustLut<Val>>::new(&filter_obj.lut, Vars::new([]));
    let results: Vec<_> = filter_obj
        .id
        .run((ctx, val))
        .map(unwrap_valr)
        .filter_map(|r| r.ok())
        .collect();

    // Format output
    if results.is_empty() {
        Ok(format!("[OK] jaq: {filter} → no results"))
    } else if results.len() == 1 {
        Ok(format!("[OK] jaq: {filter}\n\n{:#?}", results[0]))
    } else {
        let count = results.len();
        Ok(format!("[OK] jaq: {filter} → {count} results\n\n{:#?}", results))
    }
}

handler!(handle_jaq, exec_jaq);

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("jaq", ""),
        description: "JSON processor (jq-compatible). Filters JSON files with jq syntax.\nExamples:\nGet field  →  {\"filter\":\".name\",\"path\":\"package.json\"}\nArray keys  →  {\"filter\":\".dependencies | keys\",\"path\":\"Cargo.toml\"}\nArray elements  →  {\"filter\":\".[] | .id\",\"path\":\"data.json\"}\nSelect  →  {\"filter\":\".[] | select(.age > 30)\",\"path\":\"users.json\"}",
        input_schema: serde_json::json!({"type":"object","properties":{"filter":{"type":"string","description":"jq filter expression"},"path":{"type":"string","description":"JSON file path"}},"required":["filter","path"],"additionalProperties":false}),
        handler: handle_jaq,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(15),
    });
}
