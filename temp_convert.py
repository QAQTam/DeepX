import re

files = [
    'crates/deepx-tools/src/ask_user.rs',
    'crates/deepx-tools/src/file_query.rs',
    'crates/deepx-tools/src/file_mutate.rs',
    'crates/deepx-tools/src/git_tool.rs',
]

for fpath in files:
    with open(fpath, 'r', encoding='utf-8') as f:
        content = f.read()

    # 1. Function signature
    content = re.sub(
        r'fn (exec_\w+)\(args: &str\) -> String',
        r'fn \1(args: &serde_json::Value) -> ToolResult',
        content
    )

    # 2. serde_json::from_str(args) → args directly
    content = re.sub(
        r'serde_json::from_str\(args\)\.ok\(\)\s*\.and_then\(\|v: serde_json::Value\|\s*v\.get',
        r'args.get',
        content
    )
    content = re.sub(
        r'serde_json::from_str::<serde_json::Value>\(args\)\s*\.ok\(\)\s*\?\s*\.get',
        r'args.get',
        content
    )

    # 3. Wrap return format!(...) 
    content = re.sub(
        r'return (format!\()',
        r'return ToolResult::from_string(\1',
        content
    )

    with open(fpath, 'w', encoding='utf-8', newline='\n') as f:
        f.write(content)
    print(f'Done: {fpath}')
