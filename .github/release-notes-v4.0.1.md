## v4.0.1-dev

### New Features
- **Parallel tool calls**: multiple tools execute concurrently via threads (web search, fetch, etc.)
- **Lang persistence**: language choice saved across restarts
- **max_tool_rounds persistence**: configurable via F10 menu, survives restart

### Fixes
- DSML Tool Call Schema injected into system prompt (paper Table 4)
- Think Max instruction injected when effort="max"
- DSML code fence leak fixed

### Cleanup
- Removed dead deps: tokio full (dsx-tools), serde_json (dsx), ts-rs
- Removed auto_mode + phase system
- Removed prompt_lang dead field

### Binaries
- dsx.exe (6.7 MB)
- dsx-tui.exe (2.3 MB)
