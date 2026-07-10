import re, os

def convert_file(path):
    with open(path, 'r', encoding='utf-8') as f:
        content = f.read()
    original = content
    
    # Replace format!("[OK] ...") patterns
    content = re.sub(
        r'format!\("\[OK\]\s+(.*?)"(.*?)\)',
        r'crate::json_ok(serde_json::json!({"content": format!("\1"\2)}))',
        content
    )
    
    if content != original:
        with open(path, 'w', encoding='utf-8') as f:
            f.write(content)
        print(f'MODIFIED: {path}')
    else:
        print(f'SKIP: {path}')

files = [
    r'F:\DeepX\crates\deepx-tools\src\task.rs',
]
for f in files:
    if os.path.exists(f):
        convert_file(f)
