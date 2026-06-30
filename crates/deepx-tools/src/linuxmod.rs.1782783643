//! linuxmod — cross-platform micro-shell for model tool calls.
//!
//! Models write familiar shell syntax; Rust parses the pipeline,
//! routes each segment to native handlers, and connects stdio.
//!
//! Example: `grep -rl 'TODO' src/ | xargs sed -i 's/TODO/DONE/g'`
//!
//! Segments are independently safety-checked. On Linux, GNU binaries
//! are spawned via std::process::Command; on Windows, pure-Rust engines.

use crate::ToolCallCtx;
use crate::ToolResult;

// ── Pipeline splitter ──

/// Split a command string on unquoted `|` into pipeline segments.
/// `shell_words::split` handles the quote-respecting tokenisation per segment.
fn split_pipeline(input: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                // backslash: escape next char unconditionally
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '|' if !in_single && !in_double => {
                let seg = current.trim().to_string();
                if !seg.is_empty() {
                    segments.push(seg);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let seg = current.trim().to_string();
    if !seg.is_empty() {
        segments.push(seg);
    }
    segments
}

// ── Segment execution primitives ──

/// Run a single pipeline segment and return (stdout_string, success).
/// `stdin_text` is piped from the previous segment.
fn run_segment(argv: &[String], stdin_text: &str) -> (String, bool) {
    if argv.is_empty() {
        return ("[ERROR] linuxmod: empty segment".into(), false);
    }
    let cmd = &argv[0];
    let args = &argv[1..];

    match cmd.as_str() {
        "grep" => run_grep(args, stdin_text),
        "sed" => run_sed(args, stdin_text),
        "sort" => run_sort(args, stdin_text),
        "wc" => run_wc(args, stdin_text),
        "cat" => run_cat(args),
        "echo" => run_echo(args),
        "head" => run_head_tail(args, stdin_text, true),
        "tail" => run_head_tail(args, stdin_text, false),
        "cut" => run_cut(args, stdin_text),
        "jaq" | "jq" => run_jaq_segment(args, stdin_text),
        "ls" => run_ls(args),
        "xargs" => run_xargs(args, stdin_text),
        _ => (format!("[ERROR] linuxmod: unknown command '{}'", cmd), false),
    }
}

// ── Subcommand handlers ──

fn run_grep(args: &[String], stdin: &str) -> (String, bool) {
    let mut pattern = String::new();
    let mut path: Option<String> = None;
    let mut recursive = false;
    let mut line_numbers = true;
    let mut files_only = false;
    let mut glob: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-r" | "--recursive" => recursive = true,
            "-n" => line_numbers = true,
            "-l" | "--files-with-matches" => files_only = true,
            "--include" => {
                i += 1;
                if i < args.len() { glob = Some(args[i].clone()); }
            }
            a if a.starts_with("--include=") => {
                glob = Some(a[10..].to_string());
            }
            a if !a.starts_with('-') => {
                if pattern.is_empty() {
                    pattern = a.to_string();
                } else {
                    path = Some(a.to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }

    if pattern.is_empty() {
        return ("[ERROR] linuxmod grep: pattern required".into(), false);
    }

    let raw = match path {
        Some(ref p) => super::grep::exec_grep_rust(&pattern, p, recursive, line_numbers, glob.as_deref()),
        None if !stdin.is_empty() => {
            // Search stdin: filter lines matching the pattern
            let re = match regex::Regex::new(&pattern) {
                Ok(r) => r,
                Err(e) => return (format!("[ERROR] grep: invalid pattern: {e}"), false),
            };
            let results: Vec<String> = stdin
                .lines()
                .enumerate()
                .filter(|(_, l)| re.is_match(l))
                .map(|(i, l)| if line_numbers { format!("{}:{}", i + 1, l) } else { l.to_string() })
                .collect();
            if results.is_empty() {
                format!("[OK] grep: no matches for {pattern}")
            } else {
                results.join("\n")
            }
        }
        None => super::grep::exec_grep_rust(&pattern, ".", recursive, line_numbers, glob.as_deref()),
    };
    if raw.starts_with("[ERROR]") {
        return (raw, false);
    }
    if raw.starts_with("[OK] grep: no matches") {
        return (raw, true);
    }

    if files_only {
        // Extract file paths — grep output is "path:linenum:content".
        // Use regex to handle Windows paths that contain ':' (e.g. C:/...).
        let re = regex::Regex::new(r"^(.+):\d+:").unwrap();
        let mut files: Vec<&str> = raw
            .lines()
            .filter_map(|l| re.captures(l).and_then(|c| c.get(1)).map(|m| m.as_str()))
            .collect();
        // Deduplicate while preserving order
        let mut seen = std::collections::HashSet::new();
        files.retain(|f| seen.insert(*f));
        (files.join("\n"), true)
    } else {
        (raw, true)
    }
}

fn run_sed(args: &[String], stdin: &str) -> (String, bool) {
    let mut scripts: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    let mut in_place = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            a if a.starts_with("-i") => in_place = true,
            "-e" => {
                i += 1;
                if i < args.len() { scripts.push(args[i].clone()); }
            }
            a if !a.starts_with('-') => {
                if scripts.is_empty() {
                    scripts.push(a.to_string());
                } else {
                    paths.push(a.to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }

    if scripts.is_empty() {
        return ("[ERROR] linuxmod sed: script required".into(), false);
    }
    let script = scripts.join("; ");

    // File mode: path(s) given, use platform dispatch
    if !paths.is_empty() {
        let mut results = Vec::new();
        for path in &paths {
            let result = super::sed::run_sed_core(&script, path, in_place);
            results.push(result);
        }
        let output = results.join("\n");
        let ok = !output.starts_with("[ERROR]");
        return (output, ok);
    }

    // Stdin mode: apply sed to piped content (non-in-place only)
    if !stdin.is_empty() {
        if in_place {
            return ("[ERROR] linuxmod sed: -i not supported on stdin (missing file path)".into(), false);
        }
        let result = apply_sed_str(stdin, &script);
        let ok = !result.starts_with("[ERROR]");
        return (result, ok);
    }

    ("[ERROR] linuxmod sed: no input (path or stdin required)".into(), false)
}

/// Apply a sed script to piped stdin via temp file + deepx-sed.
fn apply_sed_str(input: &str, script: &str) -> String {
    super::sed::apply_sed_to_stdin(input, script)
}

fn run_sort(args: &[String], stdin: &str) -> (String, bool) {
    let mut unique = false;
    let mut reverse = false;
    let mut path: Option<String> = None;
    for a in args {
        match a.as_str() {
            "-u" | "--unique" => unique = true,
            "-r" | "--reverse" => reverse = true,
            a if !a.starts_with('-') => path = Some(a.to_string()),
            _ => {}
        }
    }

    let result = if let Some(ref p) = path {
        super::sort::run_sort_core(p, unique, reverse)
    } else if !stdin.is_empty() {
        super::sort::run_sort_str(stdin, unique, reverse)
    } else {
        return ("[ERROR] linuxmod sort: no input".into(), false);
    };

    if result.starts_with("[ERROR]") {
        (result, false)
    } else {
        (result, true)
    }
}

fn run_wc(args: &[String], stdin: &str) -> (String, bool) {
    let mut lines_only = false;
    let mut path: Option<String> = None;
    for a in args {
        match a.as_str() {
            "-l" | "--lines" => lines_only = true,
            a if !a.starts_with('-') => path = Some(a.to_string()),
            _ => {}
        }
    }

    let (content, label) = if let Some(ref p) = path {
        match std::fs::read_to_string(p) {
            Ok(c) => (c, p.clone()),
            Err(e) => return (format!("[ERROR] linuxmod wc: {e}"), false),
        }
    } else if !stdin.is_empty() {
        (stdin.to_string(), "<stdin>".into())
    } else {
        return ("[ERROR] linuxmod wc: no input".into(), false);
    };

    let result = super::wc::run_wc_core(&content, &label, lines_only);
    if result.starts_with("[ERROR]") {
        (result, false)
    } else {
        (result, true)
    }
}

fn run_cat(args: &[String]) -> (String, bool) {
    let mut results = Vec::new();
    for a in args {
        if a.starts_with('-') { continue; }
        match std::fs::read_to_string(a) {
            Ok(c) => results.push(c),
            Err(e) => results.push(format!("[ERROR] linuxmod cat: {}: {e}", a)),
        }
    }
    if results.is_empty() {
        return ("[ERROR] linuxmod cat: no files specified".into(), false);
    }
    (results.join(""), true)
}

fn run_echo(args: &[String]) -> (String, bool) {
    (args.join(" "), true)
}

fn run_head_tail(args: &[String], stdin: &str, is_head: bool) -> (String, bool) {
    let mut n: usize = 10;
    let mut path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            a if a.starts_with("-n") => {
                let val = if a.len() > 2 { &a[2..] } else {
                    i += 1;
                    if i < args.len() { args[i].as_str() } else { "" }
                };
                n = val.parse().unwrap_or(10);
            }
            a if !a.starts_with('-') => path = Some(a.to_string()),
            _ => {}
        }
        i += 1;
    }

    let label = if is_head { "head" } else { "tail" };
    let content = match path {
        Some(ref p) => match std::fs::read_to_string(p) {
            Ok(c) => c,
            Err(e) => return (format!("[ERROR] linuxmod {label}: {e}"), false),
        },
        None if !stdin.is_empty() => stdin.to_string(),
        None => return (format!("[ERROR] linuxmod {label}: path or stdin required"), false),
    };

    let lines: Vec<&str> = content.lines().collect();
    let result: String = if is_head {
        lines.iter().take(n).copied().collect::<Vec<_>>().join("\n")
    } else {
        let start = if n >= lines.len() { 0 } else { lines.len() - n };
        lines[start..].join("\n")
    };
    (result, true)
}

fn run_cut(args: &[String], stdin: &str) -> (String, bool) {
    let mut delimiter = '\t';
    let mut fields: Vec<usize> = Vec::new();
    let mut path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" => {
                i += 1;
                if i < args.len() {
                    delimiter = args[i].chars().next().unwrap_or('\t');
                }
            }
            a if a.starts_with("-f") => {
                let val = if a.len() > 2 { &a[2..] } else {
                    i += 1;
                    if i < args.len() { args[i].as_str() } else { "" }
                };
                fields = val.split(',').filter_map(|s| s.parse::<usize>().ok()).collect();
            }
            a if !a.starts_with('-') => path = Some(a.to_string()),
            _ => {}
        }
        i += 1;
    }

    let content = match path {
        Some(ref p) => match std::fs::read_to_string(p) {
            Ok(c) => c,
            Err(e) => return (format!("[ERROR] linuxmod cut: {e}"), false),
        },
        None => stdin.to_string(),
    };

    if fields.is_empty() {
        return ("[ERROR] linuxmod cut: -f fields required".into(), false);
    }

    let result: Vec<String> = content
        .lines()
        .map(|line| {
            let cols: Vec<&str> = line.split(delimiter).collect();
            fields.iter()
                .filter_map(|&f| if f > 0 && f <= cols.len() { Some(cols[f - 1]) } else { None })
                .collect::<Vec<_>>()
                .join(&delimiter.to_string())
        })
        .collect();
    (result.join("\n"), true)
}

fn run_jaq_segment(args: &[String], stdin: &str) -> (String, bool) {
    let mut filter = String::new();
    let mut path: Option<String> = None;
    for a in args {
        if a.starts_with('-') { continue; }
        if filter.is_empty() {
            filter = a.to_string();
        } else {
            path = Some(a.to_string());
        }
    }

    let input = match path {
        Some(ref p) => match std::fs::read_to_string(p) {
            Ok(c) => c,
            Err(e) => return (format!("[ERROR] linuxmod jaq: {e}"), false),
        },
        None => stdin.to_string(),
    };

    if filter.is_empty() {
        return ("[ERROR] linuxmod jaq: filter required".into(), false);
    }

    match super::jaq::run_jaq(&filter, &input, "<stdin>") {
        Ok(s) => {
            let ok = !s.starts_with("[ERROR]");
            (s, ok)
        }
        Err(e) => (format!("[ERROR] linuxmod jaq: {e}"), false),
    }
}

fn run_ls(args: &[String]) -> (String, bool) {
    let mut name_only = false;
    let mut path = String::from(".");
    for a in args {
        match a.as_str() {
            "-1" => name_only = true,
            a if !a.starts_with('-') => path = a.to_string(),
            _ => {}
        }
    }
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            let mut lines: Vec<String> = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name_only {
                    lines.push(name);
                } else {
                    let meta = entry.metadata();
                    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                    let sz = if size > 1024 * 1024 {
                        format!("{:>6.1}M", size as f64 / 1_048_576.0)
                    } else if size > 1024 {
                        format!("{:>6}K", size / 1024)
                    } else {
                        format!("{:>6}B", size)
                    };
                    lines.push(format!("{} {}", sz, name));
                }
            }
            lines.sort();
            (lines.join("\n"), true)
        }
        Err(e) => (format!("[ERROR] linuxmod ls: {e}"), false),
    }
}

fn run_xargs(args: &[String], stdin: &str) -> (String, bool) {
    if stdin.trim().is_empty() {
        return ("[OK] xargs: empty input, nothing to do".into(), true);
    }
    if args.is_empty() {
        return ("[ERROR] linuxmod xargs: command required (e.g. xargs sed -i 's/old/new/')".into(), false);
    }

    let has_placeholder = args.iter().any(|a| !a.starts_with('-') && a.contains("{}"));

    let lines: Vec<&str> = stdin.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut results: Vec<String> = Vec::new();
    let mut all_ok = true;

    for line in &lines {
        let argv: Vec<String> = if has_placeholder {
            // Replace {} only in non-flag args (skip -I{}, -n, etc.)
            args.iter().map(|a| {
                if a.starts_with('-') { a.clone() } else { a.replace("{}", line) }
            }).collect()
        } else {
            // Default: append the line as the final argument
            let mut v: Vec<String> = args.to_vec();
            v.push(line.to_string());
            v
        };
        let (out, ok) = run_segment(&argv, "");
        if !ok { all_ok = false; }
        results.push(out);
    }

    let combined = results.join("\n");
    if combined.is_empty() {
        ("[OK] xargs: done".into(), true)
    } else {
        (combined, all_ok)
    }
}

// ── Pipeline executor ──

fn execute_pipeline(segments: &[String]) -> (String, bool) {
    if segments.is_empty() {
        return ("[ERROR] linuxmod: empty command".into(), false);
    }

    let mut prev_stdout = String::new();

    for (_idx, seg) in segments.iter().enumerate() {
        // Safety check on raw segment text
        if let crate::SafetyVerdict::Block(reason) = crate::safety::classify_execution(seg) {
            return (format!("[ERROR] linuxmod: {reason}"), false);
        }

        let argv = match shell_words::split(seg) {
            Ok(a) => a,
            Err(e) => {
                return (format!("[ERROR] linuxmod: cannot parse '{}': {e}", seg), false);
            }
        };

        let (out, ok) = run_segment(&argv, &prev_stdout);
        if !ok {
            // On error, stop the pipeline
            return (out, false);
        }
        prev_stdout = out;
    }

    (prev_stdout, true)
}

// ── Tool handler ──

pub(super) fn exec_linuxmod(args: &str) -> String {
    let command = crate::parse_arg(args, "command");
    if command.is_empty() {
        return "[ERROR] linuxmod: command required".into();
    }

    // Normalize unquoted Windows backslash-paths → forward slash.
    // Must happen before shell_words::split, which treats \ as escape.
    let command = normalize_windows_path_seps(&command);

    let segments = split_pipeline(&command);
    let (output, _success) = execute_pipeline(&segments);
    output
}

/// Replace \ with / only in unquoted segments that look like Windows paths.
/// Preserves \ inside quotes (sed patterns, regex, JSON, etc.).
fn normalize_windows_path_seps(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        if ch == '\\' && !in_single && !in_double {
            out.push('/');
        } else {
            if ch == '\'' && !in_double { in_single = !in_single; }
            if ch == '"' && !in_single { in_double = !in_double; }
            out.push(ch);
        }
    }
    out
}

use crate::handler;
handler!(handle_linuxmod, exec_linuxmod);

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(crate::ToolHandler {
        key: crate::ToolKey::new("linuxmod", ""),
        description: "Cross-platform micro-shell. Write familiar shell pipelines that run natively.\n\
\n\
Subcommands and their flags (• = reads stdin, ○ = file only):\n\
  grep •   [-r] [-n] [-l] [--include=GLOB] PATTERN [PATH]\n\
           -r: recursive, -n: line numbers (default), -l: filenames only, --include: file glob filter\n\
  sed  •   [-i] [-e] SCRIPT [PATH]\n\
           -i: in-place edit (shows unified diff), -e: script (s/old/new/flags). If PATH omitted, reads stdin.\n\
  sort •   [-u] [-r] [PATH]\n\
           -u: unique/dedup, -r: reverse. If PATH omitted, reads stdin.\n\
  wc   •   [-l] [PATH]\n\
           -l: lines only. If PATH omitted, reads stdin.\n\
  cat  ○   PATH...\n\
  echo ○   ARGS...\n\
           Prints arguments joined by spaces. Useful for debugging and with xargs.\n\
  head •   [-nN] PATH\n\
           Default -n10. If PATH omitted, reads stdin.\n\
  tail •   [-nN] PATH\n\
           Default -n10. If PATH omitted, reads stdin.\n\
  cut  •   -dDELIM -fFIELDS [PATH]\n\
           -d: delimiter char, -f: comma-separated field numbers (1-based). If PATH omitted, reads stdin.\n\
  jaq/jq • FILTER [PATH]\n\
           jq-compatible JSON filter. Outputs JSON strings. If PATH omitted, reads stdin.\n\
  ls   ○   [-1] [PATH]\n\
           -1: filename only (no sizes), suitable for piping to xargs.\n\
  xargs •  COMMAND...\n\
           Reads stdin lines and runs COMMAND for each line. Lines appended as last arg by default.\n\
           Use {} in COMMAND to control insertion position (e.g. xargs -I{} cp {} {}.bak).\n\
\n\
Pipes: use | between segments. Each segment is safety-checked independently.\n\
\n\
Notes:\n\
  - Paths: use forward slashes (C:/Users/...), not backslashes. Backslashes in patterns (sed, grep) are preserved.\n\
  - No shell expansion: $env:TEMP, ~, *, and other shell variables are NOT expanded. Use absolute paths.\n\
  - sed -i always shows a unified diff preview; sed without -i outputs to stdout.\n\
\n\
Examples:\n\
  grep -rn 'fn main' src/\n\
  grep -rl 'TODO' src/ --include='*.rs' | xargs sed -i 's/TODO/DONE/g'\n\
  cat data.txt | sort -u | head -n5\n\
  grep -rn 'fn ' src/ | wc -l\n\
  jaq '.dependencies | keys' Cargo.toml\n\
  cut -d: -f1,3 /etc/passwd | sort -u | wc -l\n\
  ls -1 dir/ | xargs grep 'pattern'",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell pipeline with subcommands: grep, sed, sort, wc, cat, echo, head, tail, cut, jaq/jq, ls, xargs. Use | for pipes. Use forward slashes in paths (C:/Users/...). Patterns inside quotes are literal (no shell expansion)."}
            },
            "required": ["command"],
            "additionalProperties": false
        }),
        handler: handle_linuxmod,
        safety: |ctx| {
            let cmd = ctx.get_str("command").unwrap_or("");
            // Per-segment safety is done in execute_pipeline; here we just block the obviously malicious.
            let lower = cmd.to_lowercase();
            if lower.contains("rm -rf /") || lower.contains("sudo ") || lower.contains("mkfs.") {
                crate::SafetyVerdict::Block(format!("linuxmod: potentially destructive: {}", cmd))
            } else {
                crate::SafetyVerdict::Allow
            }
        },
        default_timeout: std::time::Duration::from_secs(120),
    });
}
