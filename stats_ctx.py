import json, os, sys

f = os.path.expandvars(r'%USERPROFILE%\.deepx\logs\200cfa5c2fd3771c_api.json')
with open(f, 'r') as fp:
    data = json.load(fp)

last = data[-1]
req = last['req']
msgs = req.get('messages', [])

roles = {}
for m in msgs:
    r = m.get('role', '?')
    roles[r] = roles.get(r, 0) + 1

thinking_blocks = 0
tool_calls_in_asst = 0
thinking_chars = 0
tool_call_chars = 0
text_chars = 0
tool_result_chars = 0

for m in msgs:
    role = m.get('role', '')
    content = m.get('content', [])
    if isinstance(content, str):
        if role == 'tool': tool_result_chars += len(content)
        elif role in ('user', 'assistant'): text_chars += len(content)
        continue
    for c in content:
        if not isinstance(c, dict):
            continue
        if 'reasoning' in c:
            thinking_blocks += 1
            thinking_chars += len(str(c['reasoning']))
        if c.get('type') == 'tool_use' or 'name' in c:
            tool_calls_in_asst += 1
            if 'input' in c:
                tool_call_chars += len(json.dumps(c['input']))
        if 'text' in c:
            text_chars += len(str(c['text']))
        if 'content' in c and role == 'tool':
            tool_result_chars += len(str(c['content']))

tools_defined = len(req.get('tools', []))
tools_chars = len(json.dumps(req.get('tools', [])))

print(f'=== Context Breakdown ===')
print(f'Messages: {len(msgs)} (sys={roles.get("system",0)} user={roles.get("user",0)} asst={roles.get("assistant",0)} tool={roles.get("tool",0)})')
print(f'---')
print(f'Text (user+asst): {text_chars:,} chars  ~{text_chars//4:,} tokens')
print(f'Thinking: {thinking_blocks} blocks, {thinking_chars:,} chars  ~{thinking_chars//4:,} tokens')
print(f'Tool calls (asst): {tool_calls_in_asst} calls, {tool_call_chars:,} chars  ~{tool_call_chars//4:,} tokens')
print(f'Tool results: {tool_result_chars:,} chars  ~{tool_result_chars//4:,} tokens')
print(f'Tools defined: {tools_defined} tools, {tools_chars:,} chars  ~{tools_chars//4:,} tokens')
total = text_chars + thinking_chars + tool_call_chars + tool_result_chars + tools_chars
print(f'---')
print(f'Grand total: {total:,} chars  ~{total//4:,} tokens')
print()
print(f'### Pie ({total//4:,} tokens) ###')
pct = lambda n: f'{n*100/total:.0f}%' if total > 0 else '0%'
print(f'  Text:         {pct(text_chars)}')
print(f'  Thinking:     {pct(thinking_chars)}')
print(f'  Tool calls:   {pct(tool_call_chars)}')
print(f'  Tool results: {pct(tool_result_chars)}')
print(f'  Tools def:    {pct(tools_chars)}')
