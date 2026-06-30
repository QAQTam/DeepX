import re 
c = open(r'D:\project\DeepX\crates\deepx-tools\src\manager.rs', 'r', encoding='utf-8').read() 
n = open(r'D:\project\DeepX\temp_all_defs.txt', 'r', encoding='utf-8').read() 
start = c.find('    pub fn all_defs') 
end = c.find('    pub fn filtered_defs') 
c = c[:start] + n.rstrip() + '\n\n' + c[end:] 
open(r'D:\project\DeepX\crates\deepx-tools\src\manager.rs', 'w', encoding='utf-8').write(c) 
print('OK') 
