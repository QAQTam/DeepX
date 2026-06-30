import re
with open(r'D:\project\DeepX\crates\deepx-tools\src\manager.rs', 'r', encoding='utf-8') as f:
    content = f.read()
with open(r'D:\project\DeepX\temp_all_defs.txt', 'r', encoding='utf-8') as f:
    new_fn = f.read()
content = re.sub(r'    pub fn all_defs.*?\n    }\s*\n\s*pub fn filtered_defs', new_fn.rstrip() + '\n\n    pub fn filtered_defs', content, count=1, flags=re.DOTALL)
if content.find('actions_str') == -1:
    print('REGEX DID NOT MATCH - attempting brute force')
    start = content.find('    pub fn all_defs')
    end = content.find('    pub fn filtered_defs')
        content = content[:start] + new_fn.rstrip() + '\n\n' + content[end:]
        print('Brute force OK')
with open(r'D:\project\DeepX\crates\deepx-tools\src\manager.rs', 'w', encoding='utf-8') as f:
    f.write(content)
print('Done')
