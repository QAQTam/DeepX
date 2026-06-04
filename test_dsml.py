#!/usr/bin/env python3
"""DeepX DSML format test — mimics the project's prompt injection.

Tests whether DeepSeek V4, when given the DSML tool-calling instructions
but with tool_choice="none" (no native JSON tool_calls), still outputs
DSML-formatted tool call blocks in the text content.

Usage:
  export DEEPSEEK_API_KEY=sk-xxx
  python3 test_dsml.py "list files in /tmp"
  python3 test_dsml.py "read /etc/hosts" --model deepseek-v4-flash
"""

import os, sys, json, argparse
from openai import OpenAI

# ── DeepX's DSML_SCHEMA (from prompt.rs) ──
DSML_SCHEMA = """## Tools

You have access to a set of tools to help answer the user's question. You can
invoke tools by writing a "<|DSML|tool_calls>" block like the following:

<|DSML|tool_calls>
<|DSML|invoke name="$TOOL_NAME">
<|DSML|parameter name="$PARAMETER_NAME" string="true|false">$PARAMETER_VALUE
</|DSML|parameter>
...
</|DSML|invoke>
</|DSML|tool_calls>

String parameters should be specified as is and set string="true". For all
other types (numbers, booleans, arrays, objects), pass the value in JSON
format and set string="false"."""

# ── DeepX's system prompt (abbreviated from prompt.rs) ──
SYSTEM_PROMPT = """You are opencode, an interactive CLI tool that helps users with software engineering tasks.

IMPORTANT:
- You have access to tools for file reading, writing, editing, searching, and executing commands.
- Output in DSML format when you need to invoke a tool.
- After tool calls, you MUST wait for results before the next tool call.

### Available Tools

- read_file: Read a file with optional line range.
- write_file: Write content to a file.
- edit_file: Edit a file by replacing old_string with new_string.
- list_dir: List files and directories with names and sizes.
- exec: Execute a shell command.
- search: Search file contents with regex.
- explore: Analyze project structure.
"""

SYSTEM_PROMPT_WITH_DSML = SYSTEM_PROMPT + "\n" + DSML_SCHEMA

def test(user_msg: str, model: str, api_key: str, base_url: str, max_tokens: int, extra_title: str = ""):
    client = OpenAI(api_key=api_key, base_url=base_url)

    messages = [
        {"role": "system", "content": SYSTEM_PROMPT_WITH_DSML},
        {"role": "user", "content": user_msg},
    ]

    print(f"{'='*70}")
    print(f"Model: {model} | max_tokens: {max_tokens} | tool_choice: none")
    print(f"User: {user_msg}")
    if extra_title:
        print(f"Extra: {extra_title}")
    print(f"{'='*70}")

    response = client.chat.completions.create(
        model=model,
        messages=messages,
        stream=True,
        max_tokens=max_tokens,
        reasoning_effort="high",
        extra_body={"thinking": {"type": "enabled"}},
        tool_choice="none",
    )

    reasoning = ""
    content = ""
    for chunk in response:
        delta = chunk.choices[0].delta if chunk.choices else None
        if delta is None:
            continue
        if getattr(delta, "reasoning_content", None):
            chunk_text = delta.reasoning_content
            reasoning += chunk_text
            sys.stderr.write(f"\x1b[90m{chunk_text}\x1b[0m")
            sys.stderr.flush()
        elif delta.content:
            content += delta.content
            sys.stdout.write(delta.content)
            sys.stdout.flush()

    print()
    print(f"{'='*70}")
    has_dsml = "｜DSML｜" in content or "tool_calls" in content
    print(f"DSML blocks detected: {has_dsml}")
    if has_dsml:
        # Highlight DSML blocks
        for line in content.split("\n"):
            if "｜DSML｜" in line or ("<" in line and "tool_calls" in line):
                print(f"  \x1b[32m>>> {line}\x1b[0m")
    print(f"Content length: {len(content)} chars")
    if reasoning:
        print(f"Reasoning length: {len(reasoning)} chars")
    print()

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Test DeepSeek DSML format output")
    parser.add_argument("query", nargs="?", default="list the files in /tmp",
                        help="User message to send")
    parser.add_argument("--model", default="deepseek-v4-pro",
                        help="Model ID (default: deepseek-v4-pro)")
    parser.add_argument("--api-key", default=os.environ.get("DEEPSEEK_API_KEY", ""),
                        help="API key (default: $DEEPSEEK_API_KEY)")
    parser.add_argument("--base-url", default="https://api.deepseek.com",
                        help="Base URL")
    parser.add_argument("--max-tokens", type=int, default=4096,
                        help="Max output tokens")
    parser.add_argument("--no-tools", action="store_true",
                        help="Remove tool definitions from system prompt")
    parser.add_argument("--skip-dsml", action="store_true",
                        help="Remove DSML schema from system prompt")
    args = parser.parse_args()

    if not args.api_key:
        print("ERROR: Set DEEPSEEK_API_KEY env var or pass --api-key")
        sys.exit(1)

    prompt = SYSTEM_PROMPT_WITH_DSML
    if args.no_tools:
        prompt = "You are a helpful assistant."
    elif args.skip_dsml:
        prompt = SYSTEM_PROMPT

    test(args.query, args.model, args.api_key, args.base_url, args.max_tokens)
