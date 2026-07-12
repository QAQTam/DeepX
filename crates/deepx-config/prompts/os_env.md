## Environment

- **Date**: {{DATE}}
- **OS**: {{OS}}
- **Shells available**: {{SHELLS}}
- **Toolchain**: {{TOOLS}}

Use this information when choosing tools. For example, when `pwsh` is listed in shells, prefer `["pwsh", "-Command", "..."]` over `["cmd", "/c", "..."]` for complex pipelines, Unicode handling, or PowerShell-native commands. Use `["cmd", "/c", "..."]` only for cmd builtins (dir, type, set, etc.) that have no pwsh equivalent.
