// Copyright (c) 2026 Red Authors
// License: MIT
//

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};

use crate::errors::{Result, SedError};
use encoding_rs::Encoding;

pub mod constants;
pub mod context;
mod engine;
pub mod errors;
mod fileio; // I/O utilities (encoding, file reading, in-place editing)
pub mod mbcs; // Multibyte character set support
pub mod parser;
pub mod posix_rules;
pub mod regex;
#[cfg(feature = "selinux")]
pub mod selinux; // SELinux security context support
pub mod signals;
mod util; // Custom regex engine with zero external dependencies
mod validation; // Centralized POSIX compliance rules

pub use context::{Context, PosixMode};
pub use engine::{
    apply_commands_with_context, AddressEvaluator, Command, CommandResult, ExecutionContext,
};
pub use validation::validate_config;

use engine::Command as RuntimeCommand;
use parser::Command as PC;
use parser::{AddressRange, HasAddressRange, Parser as ScriptParser};
use util::regex::{compile_regex, compile_regex_with_replacement};
use util::symlink::resolve_symlink_chain;
use util::version::compare_versions;

// detect_encoding moved to io/encoding.rs

/// Configuration for running sed commands
/// Contains all script texts, input files, and command-line flags
#[derive(Debug, Clone)]
pub struct RunConfig {
    /// sed scripts with raw bytes and their sources (for error reporting)
    /// Tuple: (converted_string, raw_bytes, source)
    /// raw_bytes is needed for accurate multibyte delimiter detection
    pub scripts_with_sources: Vec<(String, Vec<u8>, errors::ScriptSource)>,
    /// Input files to process (empty for stdin)
    pub input_files: Vec<String>,
    /// Suppress automatic printing of pattern space (-n flag)
    pub quiet: bool,
    /// Edit files in-place with optional backup suffix (-i flag)
    pub in_place: Option<String>,
    /// Use Extended Regular Expressions (-E/-r flag)
    pub extended_regex: bool,
    /// Treat files independently, reset line numbers for each (-s flag)
    pub separate_files: bool,
    /// Line length for l command formatting
    pub line_length: usize,
    /// Flush output after each line (-u flag)
    pub unbuffered: bool,
    /// POSIX mode - strict standards compliance (--posix flag OR POSIXLY_CORRECT env var)
    pub posix: bool,
    /// Strict POSIX mode - only --posix flag (not POSIXLY_CORRECT)
    pub strict_posix: bool,
    /// Follow symlinks when editing in-place (--follow-symlinks flag)
    pub follow_symlinks: bool,
    /// Sandbox mode - disable external file operations (--sandbox flag)
    pub sandbox: bool,
    /// Use NUL as line separator instead of newline (-z flag)
    pub null_data: bool,
    /// Binary mode - disable CRLF conversion on Windows (-b flag)
    pub binary: bool,
}

fn scripts_request_quiet(scripts: &[(String, Vec<u8>, errors::ScriptSource)]) -> bool {
    // Only the FIRST script can activate quiet mode with #n
    scripts
        .first()
        .map(|(s, _, _)| s.starts_with("#n"))
        .unwrap_or(false)
}

// read_all_lines moved to fileio/lines.rs
use fileio::read_all_lines;

/// Macro to handle range inheritance for commands that need end-of-range semantics.
///
/// When a command (like Change or Read) has no explicit range but is inside a brace group
/// with a range, it should inherit that group's range. This macro eliminates the duplicated
/// logic for this pattern.
macro_rules! inherit_group_range {
    ($variant:ident, $range:expr, $negated:expr, $field_name:ident, $field_value:expr, $nearest:expr, $out:expr) => {
        if $range.is_none() {
            if let Some((r, n)) = $nearest.clone() {
                $out.push(PC::$variant {
                    range: Some(r),
                    negated: $negated ^ n,
                    $field_name: $field_value,
                });
            } else {
                $out.push(PC::$variant {
                    range: $range,
                    negated: $negated,
                    $field_name: $field_value,
                });
            }
        } else {
            $out.push(PC::$variant {
                range: $range,
                negated: $negated,
                $field_name: $field_value,
            });
        }
    };
}

fn flatten_parser_commands(cmds: Vec<parser::Command>) -> Vec<parser::Command> {
    // We implement brace groups using guard branches to ensure correct logical AND semantics
    // between group conditions and inner command addresses. For commands that require
    // end-of-range semantics (like `c` and `r`), when they lack their own range we inherit the
    // nearest enclosing group's range so the engine can apply range-aware behavior.

    fn next_label(counter: &mut usize, suffix: &str) -> String {
        let id = *counter;
        *counter += 1;
        format!("__red_group_{}_{}", suffix, id)
    }

    fn rec(
        cmd: PC,
        // Closest enclosing group range for range-sensitive commands (Change/Read)
        nearest_group_range: &Option<(AddressRange, bool)>,
        out: &mut Vec<PC>,
        label_counter: &mut usize,
    ) {
        match cmd {
            PC::Group {
                range,
                negated,
                commands,
            } => {
                let end_label = next_label(label_counter, "end");

                // Guard: if NOT(group_condition) -> branch to end_label
                // Using evaluate_with_negation semantics, complement by flipping `negated`.
                out.push(PC::Branch {
                    range: range.clone(),
                    negated: !negated,
                    label: end_label.clone(),
                });

                // Determine new nearest range for range-sensitive commands
                let new_nearest = range
                    .clone()
                    .map(|r| (r, negated))
                    .or_else(|| nearest_group_range.clone());

                for c in commands {
                    rec(c, &new_nearest, out, label_counter);
                }

                out.push(PC::Label { name: end_label });
            }
            PC::Change {
                range,
                negated,
                text,
            } => {
                inherit_group_range!(Change, range, negated, text, text, nearest_group_range, out);
            }
            PC::Read {
                range,
                negated,
                path,
            } => {
                inherit_group_range!(Read, range, negated, path, path, nearest_group_range, out);
            }
            other => {
                // For all other commands, do not inherit group range; the guard already enforces
                // group condition, preserving correct logical AND with command's own address.
                out.push(other);
            }
        }
    }

    let mut out: Vec<PC> = Vec::new();
    let mut counter: usize = 0;
    let none_nearest: Option<(AddressRange, bool)> = None;
    for c in cmds {
        rec(c, &none_nearest, &mut out, &mut counter);
    }
    out
}

/// Validate all regex patterns in address range during parsing
fn validate_address_regexes(
    range: &Option<AddressRange>,
    extended_regex: bool,
    posix: bool,
) -> Result<()> {
    if let Some(addr_range) = range {
        // Validate start address
        if let Some(parser::Address::Regex(pattern)) = &addr_range.start {
            compile_regex(
                pattern,
                extended_regex,
                false,
                false,
                false,
                posix,
                "address pattern",
            )?;
        }
        // Validate end address
        if let Some(parser::Address::Regex(pattern)) = &addr_range.end {
            compile_regex(
                pattern,
                extended_regex,
                false,
                false,
                false,
                posix,
                "address pattern",
            )?;
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// DeepX integration API
// ═══════════════════════════════════════════════════════════════

use crate::errors::ScriptSource;

/// Execute a sed script against a file and return DeepX-formatted output.
///
/// For simple `s/pattern/replacement/flags` scripts, uses inline regex.
/// For complex scripts (`d`, `a\`, `:label`, etc.), delegates to the full
/// red engine (pure Rust, zero external binary).
///
/// Returns:
/// - In-place: `[OK] sed <script>\n\n<unified diff>` or `[OK] sed <script> — no changes`
/// - Dry-run: the replaced text, or `[OK] sed <script> (no output)`
pub fn deepx_run_sed(script: &str, path: &str, in_place: bool, quiet: bool) -> String {
    // Try fast path: inline regex for simple s///
    if let Some(result) = try_deepx_inline(script, path, in_place, quiet) {
        return result;
    }

    // Full engine path
    let desc = script.to_string();
    let before = if in_place {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };

    let config = RunConfig {
        scripts_with_sources: vec![(script.to_string(), script.as_bytes().to_vec(), ScriptSource::Expression(0))],
        input_files: vec![path.to_string()],
        quiet,
        in_place: if in_place { Some(String::new()) } else { None },
        extended_regex: false,
        separate_files: false,
        line_length: 70,
        unbuffered: false,
        posix: false,
        strict_posix: false,
        follow_symlinks: false,
        sandbox: false,
        null_data: false,
        binary: false,
    };

    match run(config) {
        Ok(()) => {
            if in_place {
                let after = std::fs::read_to_string(path).unwrap_or_default();
                let diff = unified_diff(&before, &after, path);
                if diff.is_empty() {
                    format!("[OK] sed {} — no changes", desc)
                } else {
                    format!("[OK] sed {}\n\n{}", desc, diff)
                }
            } else {
                // Output went to stdout — can't easily capture from here.
                // Fall through to binary path would be needed, but for dry-run
                // we already handled s/// via inline above.
                format!("[OK] sed {} (use --in-place for complex scripts)", desc)
            }
        }
        Err(e) => format!("[ERROR] sed: {e}"),
    }
}

/// Inline regex fast path for simple s/pattern/replacement/flags.
fn try_deepx_inline(script: &str, path: &str, in_place: bool, quiet: bool) -> Option<String> {
    let delim = script.chars().nth(1)?;
    if !delim.is_ascii_punctuation() { return None; }
    let parts: Vec<&str> = script[2..].splitn(3, delim).collect();
    if parts.len() < 2 { return None; }
    let pattern = parts[0];
    let replacement = parts[1];
    let flags = parts.get(2).copied().unwrap_or("");
    if !flags.chars().all(|c| matches!(c, 'g' | 'i' | 'm' | 's')) { return None; }

    let mut builder = ::regex::RegexBuilder::new(pattern);
    builder.case_insensitive(flags.contains('i'));
    builder.multi_line(flags.contains('m'));
    builder.dot_matches_new_line(flags.contains('s'));
    let re = builder.build().ok()?;

    let content = std::fs::read_to_string(path).ok()?;
    let result = if flags.contains('g') {
        re.replace_all(&content, replacement).to_string()
    } else {
        re.replace(&content, replacement).to_string()
    };

    if in_place {
        let diff = unified_diff(&content, &result, path);
        if diff.is_empty() {
            Some(format!("[OK] sed {} — no changes", script))
        } else {
            std::fs::write(path, result.as_bytes()).ok()?;
            Some(format!("[OK] sed {}\n\n{}", script, diff))
        }
    } else if quiet {
        None // quiet mode needs the full engine for p/l commands
    } else {
        Some(result)
    }
}

/// Generate a minimal unified diff (DeepX format compatible).
fn unified_diff(before: &str, after: &str, path: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "--- a/{}", path);
    let _ = writeln!(out, "+++ b/{}", path);

    let old_lines: Vec<&str> = before.lines().collect();
    let new_lines: Vec<&str> = after.lines().collect();

    // Simple diff: find first and last changed lines
    let mut first_diff = 0usize;
    while first_diff < old_lines.len() && first_diff < new_lines.len()
        && old_lines[first_diff] == new_lines[first_diff]
    {
        first_diff += 1;
    }
    let mut last_old = old_lines.len();
    let mut last_new = new_lines.len();
    while last_old > first_diff && last_new > first_diff
        && old_lines[last_old - 1] == new_lines[last_new - 1]
    {
        last_old -= 1;
        last_new -= 1;
    }

    let ctx = 3;
    let ctx_start = first_diff.saturating_sub(ctx);
    let ctx_old_end = (last_old + ctx).min(old_lines.len());
    let _ctx_new_end = (last_new + ctx).min(new_lines.len());

    let old_range = if last_old > first_diff {
        format!("{},{}", first_diff + 1, last_old)
    } else {
        format!("{}", first_diff + 1)
    };
    let new_range = if last_new > first_diff {
        format!("{},{}", first_diff + 1, last_new)
    } else {
        format!("{}", first_diff + 1)
    };
    let _ = writeln!(out, "@@ -{} +{} @@", old_range, new_range);

    // Show context before + changes + context after
    for i in ctx_start..ctx_old_end {
        if i < old_lines.len() && i >= first_diff && i < last_old {
            let _ = writeln!(out, "-{}", old_lines[i]);
        }
        if i < new_lines.len() && i >= first_diff && i < last_new {
            let _ = writeln!(out, "+{}", new_lines[i]);
        }
        if i < old_lines.len() && i >= first_diff && i < last_old
            && i < new_lines.len() && i >= first_diff && i < last_new
        {
            continue; // already printed both - and +
        }
        if i < old_lines.len() && i < first_diff || i >= last_old {
            let _ = writeln!(out, " {}", old_lines[i]);
        }
    }
    out
}

/// Compile a parser::Command::Substitution into engine::CompiledSubstitution
fn compile_substitution(
    range: Option<AddressRange>,
    negated: bool,
    pattern: String,
    pattern_raw_bytes: Option<Vec<u8>>,
    replacement: String,
    replacement_raw_bytes: Option<Vec<u8>>,
    flags: parser::SubstitutionFlags,
    delimiter: char,
    extended_regex: bool,
    posix: bool,
) -> Result<engine::CompiledSubstitution> {
    let use_last = pattern.is_empty();

    // Check for POSIX portability warnings
    use crate::util::regex::check_posix_portability;
    check_posix_portability(&replacement, posix, delimiter);

    // Parse replacement template with raw bytes handling
    use crate::util::regex::parse_replacement_with_bytes;
    let replacement_template = parse_replacement_with_bytes(
        &replacement,
        delimiter,
        posix,
        replacement_raw_bytes.as_deref(),
    );

    // Compile regex with replacement template info for smart optimization
    let regex = if !use_last {
        compile_regex_with_replacement(
            &pattern,
            extended_regex,
            flags.ignore_case,
            flags.multiline,
            flags.multiline_dotall,
            posix,
            "substitution",
            Some(&replacement_template),
            flags.occurrence.is_some(),
        )?
    } else {
        // Empty pattern means "use last regex" - create placeholder
        let never_match = regex::Matcher::compile("$.^", false, false)
            .expect("never-match pattern should always compile");
        engine::SedRegex::new(never_match)
    };

    // Literal optimization: check if both pattern and replacement are literal
    let (literal_pattern, literal_replacement) = if !use_last
        && !flags.ignore_case
        && !flags.multiline
        && !flags.multiline_dotall
        && flags.occurrence.is_none()
        && !flags.execute
    {
        if let Ok(matcher) = regex::Matcher::compile(&pattern, extended_regex, false) {
            if matcher.is_literal() {
                if let Some(lit_repl) =
                    regex::literal::to_literal_replacement(&replacement_template)
                {
                    if let Ok(compiled) = if extended_regex {
                        regex::parser::parse_ere(&pattern, posix)
                    } else {
                        regex::parser::parse_bre(&pattern, posix)
                    } {
                        if let Some(lit_pat) = regex::literal::to_literal_string(&compiled.ast) {
                            (Some(lit_pat), Some(lit_repl))
                        } else {
                            (None, None)
                        }
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    // Byte-level matching for non-UTF-8 patterns
    let (literal_pattern_bytes, literal_replacement_bytes) =
        if let (Some(pat_bytes), Some(repl_bytes)) = (&pattern_raw_bytes, &replacement_raw_bytes) {
            let has_non_ascii = pat_bytes.iter().any(|&b| b > 127);
            let is_byte_literal = !pat_bytes.iter().any(|&b| {
                matches!(
                    b,
                    b'.' | b'*'
                        | b'['
                        | b'^'
                        | b'$'
                        | b'\\'
                        | b']'
                        | b'+'
                        | b'?'
                        | b'|'
                        | b'('
                        | b')'
                )
            });
            let is_repl_simple = !repl_bytes
                .windows(2)
                .any(|w| w[0] == b'\\' && (w[1].is_ascii_digit() || w[1] == b'&'))
                && !repl_bytes.contains(&b'&');

            if has_non_ascii && is_byte_literal && is_repl_simple {
                (pattern_raw_bytes, replacement_raw_bytes)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

    Ok(engine::CompiledSubstitution {
        range,
        negated,
        pattern: regex,
        replacement: replacement_template,
        global: flags.global,
        print: flags.print,
        write_file: flags.write_file,
        occurrence: flags.occurrence,
        use_last,
        execute: flags.execute,
        print_timing: flags.print_timing,
        literal_pattern,
        literal_replacement,
        literal_pattern_bytes,
        literal_replacement_bytes,
    })
}

fn convert_new_command_to_old(
    new_cmd: parser::Command,
    extended_regex: bool,
    posix: bool,
) -> Result<Option<RuntimeCommand>> {
    // Validate address regexes first (to catch errors during parsing with proper context)
    // Extract range from command and validate any regex addresses
    let range_to_validate: Option<&Option<AddressRange>> = match &new_cmd {
        parser::Command::Substitution { range, .. } => Some(range),
        parser::Command::Delete { range, .. } => Some(range),
        parser::Command::Print { range, .. } => Some(range),
        parser::Command::Quit { range, .. } => Some(range),
        parser::Command::QuitSilent { range, .. } => Some(range),
        parser::Command::List { range, .. } => Some(range),
        parser::Command::Append { range, .. } => Some(range),
        parser::Command::Insert { range, .. } => Some(range),
        parser::Command::Change { range, .. } => Some(range),
        parser::Command::Exchange { range, .. } => Some(range),
        parser::Command::Branch { range, .. } => Some(range),
        parser::Command::Test { range, .. } => Some(range),
        parser::Command::Execute { range, .. } => Some(range),
        _ => None, // For commands we don't explicitly match, skip validation (they'll be validated at runtime)
    };
    if let Some(range) = range_to_validate {
        validate_address_regexes(range, extended_regex, posix)?;
    }

    match new_cmd {
        parser::Command::Substitution {
            range,
            negated,
            pattern,
            pattern_raw_bytes,
            replacement,
            replacement_raw_bytes,
            flags,
            delimiter,
        } => {
            let compiled = compile_substitution(
                range,
                negated,
                pattern,
                pattern_raw_bytes,
                replacement,
                replacement_raw_bytes,
                flags,
                delimiter,
                extended_regex,
                posix,
            )?;
            Ok(Some(RuntimeCommand::Substitution(compiled)))
        }
        parser::Command::Print { range, negated, .. } => {
            Ok(Some(RuntimeCommand::Print { range, negated }))
        }
        parser::Command::PrintFirstLine { range, negated, .. } => {
            Ok(Some(RuntimeCommand::PrintFirstLine { range, negated }))
        }
        parser::Command::Delete { range, negated, .. } => {
            Ok(Some(RuntimeCommand::Delete { range, negated }))
        }
        parser::Command::Quit {
            range,
            negated,
            exit_code,
            ..
        } => Ok(Some(RuntimeCommand::Quit {
            range,
            negated,
            exit_code,
        })),
        parser::Command::QuitSilent {
            range,
            negated,
            exit_code,
            ..
        } => Ok(Some(RuntimeCommand::QuitSilent {
            range,
            negated,
            exit_code,
        })),
        parser::Command::Append {
            range,
            negated,
            text,
        } => Ok(Some(RuntimeCommand::Append {
            range,
            negated,
            text,
        })),
        parser::Command::Insert {
            range,
            negated,
            text,
        } => Ok(Some(RuntimeCommand::Insert {
            range,
            negated,
            text,
        })),
        parser::Command::Change {
            range,
            negated,
            text,
        } => Ok(Some(RuntimeCommand::Change {
            range,
            negated,
            text,
        })),
        parser::Command::N { range, negated, .. } => Ok(Some(RuntimeCommand::N { range, negated })),
        parser::Command::BigD { range, negated, .. } => {
            Ok(Some(RuntimeCommand::BigD { range, negated }))
        }
        parser::Command::HoldCopy { range, negated, .. } => {
            Ok(Some(RuntimeCommand::HoldCopy { range, negated }))
        }
        parser::Command::HoldAppend { range, negated, .. } => {
            Ok(Some(RuntimeCommand::HoldAppend { range, negated }))
        }
        parser::Command::GetCopy { range, negated, .. } => {
            Ok(Some(RuntimeCommand::GetCopy { range, negated }))
        }
        parser::Command::GetAppend { range, negated, .. } => {
            Ok(Some(RuntimeCommand::GetAppend { range, negated }))
        }
        parser::Command::Exchange { range, negated, .. } => {
            Ok(Some(RuntimeCommand::Exchange { range, negated }))
        }
        parser::Command::Label { name } => Ok(Some(RuntimeCommand::Label { name })),
        parser::Command::Branch {
            range,
            negated,
            label,
        } => Ok(Some(RuntimeCommand::Branch {
            range,
            negated,
            label,
            target_index: None, // Resolved later
        })),
        parser::Command::Test {
            range,
            negated,
            label,
        } => Ok(Some(RuntimeCommand::Test {
            range,
            negated,
            label,
            target_index: None, // Resolved later
        })),
        parser::Command::TestNeg {
            range,
            negated,
            label,
        } => Ok(Some(RuntimeCommand::TestNeg {
            range,
            negated,
            label,
            target_index: None, // Resolved later
        })),
        parser::Command::Execute {
            range,
            negated,
            command,
        } => Ok(Some(RuntimeCommand::Execute {
            range,
            negated,
            command,
        })),
        parser::Command::Version { version } => {
            // v command is a compile-time check, not a runtime command
            // Check version and fail if required version is newer than ours
            let required_version = if version.is_empty() {
                "4.0"
            } else {
                version.as_str()
            };

            // Simple version comparison (GNU sed uses strverscmp)
            if compare_versions(required_version, constants::GNU_SED_COMPAT_VERSION).is_gt() {
                return Err(SedError::parse("expected newer version of sed"));
            }

            // v command doesn't generate runtime command
            Ok(None)
        }
        parser::Command::Clear { range, negated } => {
            Ok(Some(RuntimeCommand::Clear { range, negated }))
        }
        parser::Command::PrintFilename { range, negated } => {
            Ok(Some(RuntimeCommand::PrintFilename { range, negated }))
        }
        parser::Command::Next => Ok(Some(RuntimeCommand::Next)),
        parser::Command::Write {
            range,
            negated,
            path,
        } => Ok(Some(RuntimeCommand::Write {
            range,
            negated,
            path,
        })),
        parser::Command::WriteFirstLine {
            range,
            negated,
            path,
        } => Ok(Some(RuntimeCommand::WriteFirstLine {
            range,
            negated,
            path,
        })),
        parser::Command::Read {
            range,
            negated,
            path,
        } => Ok(Some(RuntimeCommand::Read {
            range,
            negated,
            path,
        })),
        parser::Command::ReadLine {
            range,
            negated,
            path,
        } => Ok(Some(RuntimeCommand::ReadLine {
            range,
            negated,
            path,
        })),
        parser::Command::LineNumber { range, negated } => {
            Ok(Some(RuntimeCommand::LineNumber { range, negated }))
        }
        parser::Command::Translate {
            range,
            negated,
            from,
            to,
            from_bytes,
            to_bytes,
            ..
        } => Ok(Some(RuntimeCommand::Translate {
            range,
            negated,
            from,
            to,
            from_bytes,
            to_bytes,
        })),
        parser::Command::List {
            range,
            negated,
            line_length,
        } => Ok(Some(RuntimeCommand::List {
            range: range.clone(),
            negated,
            line_length,
        })),
        parser::Command::Comment(_) => Ok(None),
        _ => {
            return Err(SedError::parse(format!(
                "unsupported command in script: {:?}",
                new_cmd
            )));
        }
    }
}

fn parse_scripts_to_commands(
    scripts: &[(String, Vec<u8>, errors::ScriptSource)],
    ctx: &Context,
) -> Result<Vec<RuntimeCommand>> {
    let extended_regex = ctx.extended_regex;
    let strict_posix = ctx.is_strict_posix();
    let mut commands: Vec<RuntimeCommand> = Vec::new();

    // Preprocess: merge incomplete a/i/c commands with next script (GNU extension)
    // Tuple: (string, raw_bytes, source)
    let mut merged_scripts: Vec<(String, Vec<u8>, errors::ScriptSource)> = Vec::new();
    let mut i = 0;
    while i < scripts.len() {
        let (script, raw_bytes, source) = &scripts[i];
        let trimmed = script.trim_end();

        // Check if script ends with a\, i\, or c\ (incomplete text command)
        if !strict_posix
            && (trimmed.ends_with("a\\") || trimmed.ends_with("i\\") || trimmed.ends_with("c\\"))
        {
            if i + 1 < scripts.len() {
                // Merge with next script: a\ + \n + next_script
                let mut merged = script.clone();
                merged.push('\n');
                merged.push_str(&scripts[i + 1].0);
                // Merge raw bytes too
                let mut merged_bytes = raw_bytes.clone();
                merged_bytes.push(b'\n');
                merged_bytes.extend_from_slice(&scripts[i + 1].1);
                merged_scripts.push((merged, merged_bytes, source.clone()));
                i += 2;
                continue;
            }
        }

        merged_scripts.push((script.clone(), raw_bytes.clone(), source.clone()));
        i += 1;
    }

    // Track last_regex across script parses for empty regex support
    let mut last_regex: Option<String> = None;

    for (script_str, raw_bytes, source) in merged_scripts.iter() {
        let error_context = source.to_error_context();
        let (parsed_commands, new_last_regex) = ScriptParser::parse_script_with_raw_bytes_chained(
            script_str, raw_bytes, ctx, last_regex,
        )
        .map_err(|e| e.with_context(error_context.clone()))?;
        last_regex = new_last_regex;
        let flattened = flatten_parser_commands(parsed_commands);

        // Validate POSIX compatibility if in strict POSIX mode (before conversion)
        if strict_posix {
            validate_posix_parser_commands(&flattened)
                .map_err(|e| e.with_context(error_context.clone()))?;
        }

        for new_cmd in flattened {
            if let Some(old_cmd) = convert_new_command_to_old(new_cmd, extended_regex, strict_posix)
                .map_err(|e| e.with_context(error_context.clone()))?
            {
                commands.push(old_cmd);
            }
        }
    }
    Ok(commands)
}

fn validate_labels(commands: &[RuntimeCommand]) -> Result<()> {
    let mut defined: HashSet<&str> = HashSet::new();
    for cmd in commands {
        if let RuntimeCommand::Label { name } = cmd {
            defined.insert(name.as_str());
        }
    }
    for cmd in commands {
        match cmd {
            RuntimeCommand::Branch { label, .. }
            | RuntimeCommand::Test { label, .. }
            | RuntimeCommand::TestNeg { label, .. } => {
                if !label.is_empty() && !defined.contains(label.as_str()) {
                    return Err(SedError::runtime(format!("undefined label '{}'", label)));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Pre-resolve branch labels to indices for faster execution.
/// This eliminates HashMap lookups during script execution.
fn resolve_branch_labels(commands: &mut [RuntimeCommand]) {
    // Build label -> index map
    let mut label_to_index: HashMap<String, usize> = HashMap::new();
    for (idx, cmd) in commands.iter().enumerate() {
        if let RuntimeCommand::Label { name } = cmd {
            label_to_index.insert(name.clone(), idx);
        }
    }

    // Resolve target indices for branch commands
    for cmd in commands.iter_mut() {
        match cmd {
            RuntimeCommand::Branch {
                label,
                target_index,
                ..
            } => {
                if label.is_empty() {
                    // Empty label means branch to end of script
                    *target_index = None;
                } else if let Some(&idx) = label_to_index.get(label) {
                    *target_index = Some(idx);
                }
            }
            RuntimeCommand::Test {
                label,
                target_index,
                ..
            } => {
                if label.is_empty() {
                    *target_index = None;
                } else if let Some(&idx) = label_to_index.get(label) {
                    *target_index = Some(idx);
                }
            }
            RuntimeCommand::TestNeg {
                label,
                target_index,
                ..
            } => {
                if label.is_empty() {
                    *target_index = None;
                } else if let Some(&idx) = label_to_index.get(label) {
                    *target_index = Some(idx);
                }
            }
            _ => {}
        }
    }
}

/// Validate that commands are compatible with POSIX mode (called at parse time)
fn validate_posix_parser_commands(commands: &[parser::Command]) -> Result<()> {
    for cmd in commands {
        // Check address ranges for GNU extensions using trait method
        if let Some(range) = cmd.address_range() {
            // Check start address
            if let Some(addr) = &range.start {
                validate_posix_address(&addr)?;
            }
            // Check end address
            if let Some(addr) = &range.end {
                validate_posix_address(&addr)?;
            }
        }

        match cmd {
            // GNU extension commands forbidden in POSIX mode
            parser::Command::Execute { .. } => {
                return Err(SedError::parse("unknown command: 'e'"));
            }
            parser::Command::PrintFilename { .. } => {
                return Err(SedError::parse("unknown command: 'F'"));
            }
            parser::Command::Clear { .. } => {
                return Err(SedError::parse("unknown command: 'z'"));
            }
            parser::Command::QuitSilent { .. } => {
                return Err(SedError::parse("unknown command: 'Q'"));
            }
            parser::Command::TestNeg { .. } => {
                return Err(SedError::parse("unknown command: 'T'"));
            }
            parser::Command::ReadLine { .. } => {
                return Err(SedError::parse("unknown command: 'R'"));
            }
            parser::Command::WriteFirstLine { .. } => {
                return Err(SedError::parse("unknown command: 'W'"));
            }
            parser::Command::Substitution { .. } => {
                // POSIX validation for flags is now handled in the parser
            }
            parser::Command::List { line_length, .. } => {
                // Check for GNU extension: l with numeric argument
                if line_length.is_some() {
                    return Err(SedError::parse_at("extra characters after command", 2));
                }
            }
            parser::Command::Group { commands, .. } => {
                // Recursively check group commands
                validate_posix_parser_commands(commands)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_posix_address(addr: &parser::Address) -> Result<()> {
    match addr {
        parser::Address::Line(0) => {
            // Address 0 is a GNU extension (char 6 assumes format "0,/A/p")
            Err(SedError::parse_at("invalid usage of line address 0", 6))
        }
        parser::Address::Relative(_, _) => {
            // Relative addresses (addr,+N) are GNU extensions
            Err(SedError::parse_at("unexpected ','", 3))
        }
        parser::Address::Step(_, _) => {
            // Step addresses (addr,~N) are GNU extensions
            Err(SedError::parse_at("unexpected ','", 3))
        }
        _ => Ok(()),
    }
}

/// Validate that commands are compatible with sandbox mode
fn validate_sandbox_commands(commands: &[RuntimeCommand]) -> Result<()> {
    for cmd in commands {
        match cmd {
            // Commands forbidden in sandbox mode (file I/O and command execution)
            RuntimeCommand::Execute { .. } => {
                return Err(SedError::parse("e/r/w commands disabled in sandbox mode"));
            }
            RuntimeCommand::Read { .. } => {
                return Err(SedError::parse("e/r/w commands disabled in sandbox mode"));
            }
            RuntimeCommand::ReadLine { .. } => {
                return Err(SedError::parse("e/r/w commands disabled in sandbox mode"));
            }
            RuntimeCommand::Write { .. } => {
                return Err(SedError::parse("e/r/w commands disabled in sandbox mode"));
            }
            RuntimeCommand::WriteFirstLine { .. } => {
                return Err(SedError::parse("e/r/w commands disabled in sandbox mode"));
            }
            // Substitution with execute flag or write_file option
            RuntimeCommand::Substitution(subst)
                if subst.execute || subst.write_file.is_some() =>
            {
                return Err(SedError::parse("e/r/w commands disabled in sandbox mode"));
            }
            _ => {}
        }
    }
    Ok(())
}

// expand_backup_suffix moved to fileio/inplace.rs
use fileio::expand_backup_suffix;

/// Process a single file for in-place editing
fn process_single_file(
    file_path: &str,
    commands: &[RuntimeCommand],
    quiet_mode: bool,
    extended_regex: bool,
    line_length: usize,
    unbuffered: bool,
    null_data: bool,
    posix: bool,
    binary: bool,
    follow_symlinks: bool,
    backup_suffix: Option<&str>,
) -> Result<()> {
    // Check if file is a symlink
    let meta = std::fs::symlink_metadata(file_path)
        .map_err(|e| SedError::io("can't read", file_path, e))?;

    let is_symlink = meta.file_type().is_symlink();

    // Determine the actual file to read from (resolve symlinks)
    let read_from_path = if is_symlink {
        // Use strict mode for in-place editing (returns errors)
        let resolved = resolve_symlink_chain(std::path::Path::new(file_path), true)?;
        resolved
            .to_str()
            .ok_or_else(|| SedError::runtime("symlink target path is not valid UTF-8"))?
            .to_string()
    } else {
        file_path.to_string()
    };

    // Determine output path:
    // - With --follow-symlinks: write to resolved target
    // - Without --follow-symlinks: write to original path (replacing symlink with regular file)
    let write_to_path = if follow_symlinks {
        read_from_path.clone()
    } else {
        file_path.to_string()
    };

    let file_to_read = read_from_path.as_str();
    let file_to_write = write_to_path.as_str();

    // Capture SELinux context before processing
    // When follow_symlinks is true, we get context from resolved target (getfilecon)
    // When follow_symlinks is false, we get context from original path/symlink (lgetfilecon)
    #[cfg(feature = "selinux")]
    let selinux_context = selinux::get_context(
        std::path::Path::new(if follow_symlinks {
            file_to_read
        } else {
            file_path
        }),
        follow_symlinks,
    );

    // Get file metadata and check file type (check the actual file we read from)
    let file_metadata =
        std::fs::metadata(file_to_read).map_err(|e| SedError::io("can't read", file_to_read, e))?;

    // Check if file is a regular file (not FIFO, device, socket, etc.)
    if !file_metadata.file_type().is_file() {
        // On Unix, check if it's a terminal (character device that is a tty)
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            let file_type = file_metadata.file_type();
            if file_type.is_char_device() {
                // Try to open and check if it's a terminal using IsTerminal trait
                if let Ok(file) = File::open(file_to_read) {
                    use std::io::IsTerminal;
                    if file.is_terminal() {
                        return Err(SedError::inplace(format!(
                            "couldn't edit {}: is a terminal",
                            file_path
                        )));
                    }
                }
            }
        }
        return Err(SedError::inplace(format!(
            "couldn't edit {}: not a regular file",
            file_path
        )));
    }

    // Preserve original file permissions (mode)
    let original_permissions = file_metadata.permissions();
    // Read the file content
    let mut file =
        File::open(file_to_read).map_err(|e| SedError::io("can't read", file_to_read, e))?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;

    // Check early if we can create temp file in the directory
    // This catches permission errors before we do any processing
    // Use file_to_write directory for temp file (handles symlink replacement case)
    let temp_path = format!("{}.red_temp_{}", file_to_write, std::process::id());

    // Register temp file for cleanup on signal
    signals::unix::register_temp_file(temp_path.clone());

    let temp_file_result = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path);

    if let Err(e) = temp_file_result {
        signals::unix::unregister_temp_file(&temp_path);
        // GNU sed outputs "couldn't open temporary file" for temp file creation errors
        return Err(SedError::inplace(format!(
            "couldn't open temporary file {}: {}",
            temp_path,
            match e.kind() {
                std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
                std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
                _ => e
                    .to_string()
                    .split(" (os error")
                    .next()
                    .unwrap_or(&e.to_string())
                    .to_string(),
            }
        )));
    }

    let temp_file = temp_file_result.unwrap();

    // Split into lines (preserving original byte representation)
    let (all_lines, all_lines_bytes, encoding, ends_with_separator) =
        split_file_content(content, null_data, binary);

    if all_lines.is_empty() {
        // Empty file - clean up temp file and just create backup if needed
        drop(temp_file);
        signals::unix::unregister_temp_file(&temp_path);
        let _ = std::fs::remove_file(&temp_path);
        if let Some(suffix) = backup_suffix {
            let backup_path = expand_backup_suffix(file_to_write, suffix);
            std::fs::copy(file_to_read, &backup_path)
                .map_err(|e| SedError::rename(file_to_read, &backup_path, e))?;
        }
        return Ok(());
    }

    // Create backup if suffix provided
    if let Some(suffix) = backup_suffix {
        let backup_path = expand_backup_suffix(file_to_write, suffix);
        std::fs::copy(file_to_read, &backup_path)
            .map_err(|e| SedError::rename(file_to_read, &backup_path, e))?;
    }

    // Use the temp file we already created and validated above
    let mut output = BufWriter::new(temp_file);

    // Execute using the same engine as normal mode
    // All lines from the same file, so create filename vector
    let filenames = vec![file_to_read.to_string(); all_lines.len()];
    let execute_result = execute_over_lines(
        commands,
        quiet_mode,
        extended_regex,
        line_length,
        unbuffered,
        null_data,
        posix,
        binary,
        &all_lines,
        &all_lines_bytes,
        &filenames,
        encoding,
        ends_with_separator,
        &mut output,
    );

    // If execution failed, clean up temp file and return error
    if let Err(e) = execute_result {
        signals::unix::unregister_temp_file(&temp_path);
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }

    // Flush and close the temporary file
    if let Err(e) = output.flush() {
        signals::unix::unregister_temp_file(&temp_path);
        let _ = std::fs::remove_file(&temp_path);
        return Err(e.into());
    }
    drop(output);

    // If we're replacing a symlink (not following it), remove the symlink first
    if is_symlink && !follow_symlinks {
        std::fs::remove_file(file_to_write)
            .map_err(|e| SedError::io("can't remove symlink", file_to_write, e))?;
    }

    // Replace original file with processed content
    std::fs::rename(&temp_path, file_to_write)
        .map_err(|e| SedError::io("can't rename", file_to_write, e))?;

    // Unregister temp file after successful rename
    signals::unix::unregister_temp_file(&temp_path);

    // Restore original permissions (best-effort)
    if let Err(err) = std::fs::set_permissions(file_to_write, original_permissions) {
        // Do not fail the whole operation due to permissions restoration issues
        eprintln!(
            "red: warning: failed to restore file permissions for {}: {}",
            file_to_write, err
        );
    }

    // Restore SELinux context (best-effort)
    #[cfg(feature = "selinux")]
    if let Some(ctx) = selinux_context {
        if let Err(err) = selinux::set_context(std::path::Path::new(file_to_write), ctx) {
            eprintln!(
                "red: warning: failed to restore SELinux context for {}: {}",
                file_to_write, err
            );
        }
    }

    Ok(())
}

// split_file_content moved to fileio/lines.rs
use fileio::split_file_content;

/// Main entry point for running sed commands on input files
/// Takes a RunConfig with scripts, input files, and flags, processes each file/stdin
/// according to sed semantics, and writes output to stdout or file (for in-place editing)
pub fn run(config: RunConfig) -> Result<()> {
    // Phase 1.3: Validate configuration before execution
    validate_config(&config)?;

    // Create unified Context from RunConfig
    // Phase 3.1: Context now integrated with Parser
    let ctx = Context::from_run_config(&config, config.scripts_with_sources.clone());

    let mut commands = parse_scripts_to_commands(&config.scripts_with_sources, &ctx)?;
    validate_labels(&commands)?;
    resolve_branch_labels(&mut commands);

    // Validate sandbox compatibility if in sandbox mode
    if config.sandbox {
        validate_sandbox_commands(&commands)?;
    }

    let quiet_by_header = scripts_request_quiet(&config.scripts_with_sources);

    // Handle in-place editing
    if let Some(backup_suffix) = config.in_place {
        let quiet_mode = config.quiet || quiet_by_header;
        let backup_opt = if backup_suffix.is_empty() {
            None
        } else {
            Some(backup_suffix.as_str())
        };
        for file_path in &config.input_files {
            process_single_file(
                file_path,
                &commands,
                quiet_mode,
                config.extended_regex,
                config.line_length,
                config.unbuffered,
                config.null_data,
                config.posix,
                config.binary,
                config.follow_symlinks,
                backup_opt,
            )?;
        }
        return Ok(());
    }

    // Handle separate files mode (-s)
    if config.separate_files && !config.input_files.is_empty() {
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());

        for file_path in &config.input_files {
            // Read each file separately
            let (file_lines, file_lines_bytes, file_filenames, encoding, ends_with_separator) =
                read_all_lines(
                    &[file_path.clone()],
                    config.null_data,
                    config.follow_symlinks,
                    config.binary,
                )?;

            // Execute with fresh context for each file (line numbers reset)
            execute_over_lines(
                &commands,
                config.quiet || quiet_by_header,
                config.extended_regex,
                config.line_length,
                config.unbuffered,
                config.null_data,
                config.posix,
                config.binary,
                &file_lines,
                &file_lines_bytes,
                &file_filenames,
                encoding,
                ends_with_separator,
                &mut out,
            )?;
        }
        Ok(())
    } else {
        // Check if unbuffered mode with stdin - need line-by-line processing to support early exit
        let is_stdin = config.input_files.is_empty()
            || (config.input_files.len() == 1 && config.input_files[0] == "-");
        if config.unbuffered && is_stdin {
            // Unbuffered stdin: read line by line to allow early quit
            let stdout = io::stdout();
            let mut out = io::BufWriter::new(stdout.lock());

            return process_stdin_line_by_line(
                &commands,
                config.quiet || quiet_by_header,
                config.extended_regex,
                config.line_length,
                config.null_data,
                config.posix,
                config.binary,
                &mut out,
            );
        }

        // Normal mode: treat all files as continuous stream
        let (all_lines, all_lines_bytes, all_filenames, encoding, ends_with_separator) =
            read_all_lines(
                &config.input_files,
                config.null_data,
                config.follow_symlinks,
                config.binary,
            )?;
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        execute_over_lines(
            &commands,
            config.quiet || quiet_by_header,
            config.extended_regex,
            config.line_length,
            config.unbuffered,
            config.null_data,
            config.posix,
            config.binary,
            &all_lines,
            &all_lines_bytes,
            &all_filenames,
            encoding,
            ends_with_separator,
            &mut out,
        )
    }
}

/// Process stdin line-by-line for unbuffered mode
/// This allows early exit on quit command without reading all input
fn process_stdin_line_by_line(
    commands: &[RuntimeCommand],
    quiet_mode: bool,
    extended_regex: bool,
    line_length: usize,
    null_data: bool,
    posix: bool,
    binary: bool,
    out: &mut dyn Write,
) -> Result<()> {
    let separator = if null_data { b'\0' } else { b'\n' };

    let mut ctx = ExecutionContext::new(
        None, // total_lines unknown in streaming mode
        quiet_mode,
        extended_regex,
        line_length,
        true, // unbuffered
        null_data,
    );
    let mut evaluator = AddressEvaluator::new(extended_regex, posix);

    let mut line_num = 0;

    // Read stdin byte-by-byte WITHOUT buffering using raw file descriptor
    // This ensures we only read exactly what we need and leave the rest for other processes
    #[cfg(unix)]
    use std::os::unix::io::FromRawFd;

    #[cfg(unix)]
    let mut stdin_raw = unsafe { std::fs::File::from_raw_fd(0) };

    #[cfg(not(unix))]
    let mut stdin_raw = io::stdin();

    let mut line_bytes: Vec<u8> = Vec::new();
    let mut encountered_quit = false;
    let mut buf = [0u8; 1];

    loop {
        // Read bytes until we hit separator or EOF
        let mut eof = false;
        loop {
            match stdin_raw.read(&mut buf) {
                Ok(0) => {
                    // EOF
                    eof = true;
                    break;
                }
                Ok(_) => {
                    let byte = buf[0];
                    if byte == separator {
                        break; // End of line
                    }
                    line_bytes.push(byte);
                }
                Err(e) => {
                    return Err(SedError::io("can't read", "-", e));
                }
            }
        }

        // If we have a line
        if !line_bytes.is_empty() {
            line_num += 1;
            ctx.set_current_line(line_num, line_bytes.clone());
            ctx.set_filename("-");

            let results = apply_commands_with_context(commands, &mut ctx, &mut evaluator, 0)?;

            let mut should_autoprint = !ctx.quiet_mode;

            for result in results {
                match result {
                    CommandResult::Continue(final_line, raw_bytes) => {
                        if should_autoprint {
                            write_output_line(
                                &final_line,
                                raw_bytes.as_deref(),
                                null_data,
                                true,
                                binary,
                                out,
                            );
                            out.flush()?;
                        }
                    }
                    CommandResult::Print(print_line, raw_bytes) => {
                        write_output_line(
                            &print_line,
                            raw_bytes.as_deref(),
                            null_data,
                            true,
                            binary,
                            out,
                        );
                        out.flush()?;
                    }
                    CommandResult::Delete => {
                        should_autoprint = false;
                    }
                    CommandResult::SuppressAutoprint => {
                        should_autoprint = false;
                    }
                    CommandResult::PrintAndContinue(line) => {
                        if null_data {
                            write!(out, "{}\0", line)?;
                        } else {
                            out.write_all(line.as_bytes())?;
                            out.write_all(line_ending(binary))?;
                        }
                        out.flush()?;
                    }
                    CommandResult::Quit(exit_code) => {
                        out.flush()?;
                        encountered_quit = true;
                        // Don't call process::exit() - just stop reading input
                        // This allows other processes in a pipeline to read remaining input
                        if let Some(code) = exit_code {
                            if code != 0 {
                                std::process::exit(code);
                            }
                        }
                        break;
                    }
                    _ => {
                        // For simplicity, we don't fully support all commands in streaming mode
                        // Commands like N (append next line) would require lookahead
                    }
                }
            }

            // Clear line buffer for next iteration
            line_bytes.clear();

            if encountered_quit {
                // Don't drop stdin - this ensures remaining data stays in the pipe
                // for other processes to read
                std::mem::forget(stdin_raw);
                return Ok(());
            }
        }

        // Break if EOF
        if eof {
            break;
        }
    }

    Ok(())
}

// Output helpers from fileio module
use fileio::{flush_output, line_ending, write_output_line};

/// Result of processing command results - determines control flow
enum ProcessedResult {
    /// Normal completion, continue to next line
    Done,
    /// Delete was encountered, skip to next line without output
    Deleted,
    /// Restart the script cycle (D command modified pattern space)
    Restart,
    /// Quit with exit code
    Quit(i32),
    /// Resume from PC after reading next line (n command)
    NextLineAndResume(usize),
    /// Append next line and resume (N command)
    AppendNextAndResume {
        resume_pc: usize,
        pattern_space: String,
    },
}

/// Result of N command handling - determines outer loop control flow
enum NCommandOutcome {
    /// Normal completion, break from script_cycle
    Complete,
    /// Break from script_cycle (EOF or delete)
    BreakCycle,
    /// Continue script_cycle (restart)
    ContinueCycle,
}

/// Handle the N command - append next lines to pattern space and resume execution.
///
/// This extracts the complex N command loop from `execute_over_lines()` to reduce nesting.
#[allow(clippy::too_many_arguments)]
fn handle_n_command(
    commands: &[RuntimeCommand],
    ctx: &mut ExecutionContext,
    evaluator: &mut AddressEvaluator,
    all_lines: &[String],
    _all_lines_bytes: &[Vec<u8>],
    all_filenames: &[String],
    null_data: bool,
    posix: bool,
    unbuffered: bool,
    binary: bool,
    suppress_autoprint: &mut bool,
    line_idx: &mut usize,
    initial_resume_pc: usize,
    initial_pattern_space: String,
    out: &mut dyn Write,
) -> Result<NCommandOutcome> {
    let mut current_resume_pc = initial_resume_pc;
    let mut current_pattern_space = initial_pattern_space;

    loop {
        if *line_idx + 1 >= all_lines.len() {
            // EOF: N quits; GNU mode prints, POSIX mode doesn't
            if !posix && !ctx.quiet_mode && !*suppress_autoprint {
                let _ = out.write_all(ctx.pattern_space.text().as_bytes());
                let _ = out.write_all(line_ending(binary));
                flush_output(out, unbuffered);
            }
            return Ok(NCommandOutcome::BreakCycle);
        }

        // Append next line to pattern space
        ctx.pattern_space.set(current_pattern_space.clone());
        *line_idx += 1;
        ctx.current_line_num = *line_idx + 1;
        ctx.set_filename(&all_filenames[*line_idx]);
        ctx.pattern_space.push(if null_data { '\0' } else { '\n' });
        ctx.pattern_space.push_str(&all_lines[*line_idx]);
        if *line_idx + 1 >= all_lines.len() {
            ctx.all_input_consumed = true;
        }

        // Resume execution
        let resumed = apply_commands_with_context(commands, ctx, evaluator, current_resume_pc)?;

        let inner_action = process_command_results(
            resumed,
            ctx,
            null_data,
            true, // N command always writes separator
            unbuffered,
            binary,
            suppress_autoprint,
            out,
        );

        match inner_action {
            ProcessedResult::Done => return Ok(NCommandOutcome::Complete),
            ProcessedResult::Deleted => return Ok(NCommandOutcome::BreakCycle),
            ProcessedResult::Restart => return Ok(NCommandOutcome::ContinueCycle),
            ProcessedResult::Quit(code) => std::process::exit(code),
            ProcessedResult::NextLineAndResume(_) => {
                // Nested n in N context - ignore
            }
            ProcessedResult::AppendNextAndResume {
                resume_pc: rp,
                pattern_space: ps,
            } => {
                // Continue with next N
                current_resume_pc = rp;
                current_pattern_space = ps;
                continue;
            }
        }
        break; // Normal completion of inner loop
    }
    Ok(NCommandOutcome::Complete)
}

/// Process command results and handle output, returning control flow action.
///
/// This unifies result handling between the main loop and nested N command loop.
/// The `suppress_autoprint` parameter is updated if SuppressAutoprint result is seen.
fn process_command_results(
    results: Vec<CommandResult>,
    ctx: &mut ExecutionContext,
    null_data: bool,
    write_separator: bool,
    unbuffered: bool,
    binary: bool,
    suppress_autoprint: &mut bool,
    out: &mut dyn Write,
) -> ProcessedResult {
    for result in results {
        match result {
            CommandResult::Continue(final_line, raw_bytes) => {
                if !ctx.quiet_mode && !*suppress_autoprint {
                    write_output_line(
                        &final_line,
                        raw_bytes.as_deref(),
                        null_data,
                        write_separator,
                        binary,
                        out,
                    );
                    flush_output(out, unbuffered);
                }
            }
            CommandResult::Print(print_line, raw_bytes) => {
                write_output_line(
                    &print_line,
                    raw_bytes.as_deref(),
                    null_data,
                    true,
                    binary,
                    out,
                );
                flush_output(out, unbuffered);
            }
            CommandResult::Delete => {
                return ProcessedResult::Deleted;
            }
            CommandResult::SuppressAutoprint => {
                *suppress_autoprint = true;
            }
            CommandResult::PrintAndContinue(line) => {
                if null_data {
                    let _ = write!(out, "{}\0", line);
                } else {
                    let _ = out.write_all(line.as_bytes());
                    let _ = out.write_all(line_ending(binary));
                }
                flush_output(out, unbuffered);
            }
            CommandResult::Quit(exit_code) => {
                let _ = out.flush();
                return ProcessedResult::Quit(exit_code.unwrap_or(0));
            }
            CommandResult::Restart => {
                return ProcessedResult::Restart;
            }
            CommandResult::RestartWith(new_ps) => {
                ctx.pattern_space.set(new_ps);
                return ProcessedResult::Restart;
            }
            CommandResult::RestartWithBytes(bytes) => {
                ctx.pattern_space.set_raw(bytes);
                return ProcessedResult::Restart;
            }
            CommandResult::AppendNextAndResume {
                resume_pc,
                pattern_space,
            } => {
                return ProcessedResult::AppendNextAndResume {
                    resume_pc,
                    pattern_space,
                };
            }
            CommandResult::NextLineAndResume { resume_pc } => {
                return ProcessedResult::NextLineAndResume(resume_pc);
            }
        }
    }

    ProcessedResult::Done
}

/// Execute the parsed commands over provided lines, writing results into `out`.
fn execute_over_lines(
    commands: &[RuntimeCommand],
    quiet_mode: bool,
    extended_regex: bool,
    line_length: usize,
    unbuffered: bool,
    null_data: bool,
    posix: bool,
    binary: bool,
    all_lines: &Vec<String>,
    all_lines_bytes: &Vec<Vec<u8>>,
    all_filenames: &Vec<String>,
    _encoding: &'static Encoding,
    ends_with_separator: bool,
    out: &mut dyn Write,
) -> Result<()> {
    let total_lines = if all_lines.is_empty() {
        None
    } else {
        Some(all_lines.len())
    };
    let mut ctx = ExecutionContext::new(
        total_lines,
        quiet_mode,
        extended_regex,
        line_length,
        unbuffered,
        null_data,
    );
    let mut evaluator = AddressEvaluator::new(extended_regex, posix);

    let mut line_idx: usize = 0;
    while line_idx < all_lines.len() {
        let line_num = line_idx + 1;
        ctx.set_current_line(line_num, all_lines_bytes[line_idx].clone());
        ctx.set_filename(&all_filenames[line_idx]);

        // Check if this is the last line and whether to write separator
        let is_last_line = line_idx == all_lines.len() - 1;
        let should_write_separator = !is_last_line || ends_with_separator;
        let mut resume_from_pc: Option<usize> = None;

        'script_cycle: loop {
            let next_pc: usize = resume_from_pc.take().unwrap_or(0);
            let results =
                apply_commands_with_context(&commands, &mut ctx, &mut evaluator, next_pc)?;

            let mut suppress_autoprint = false;
            let action = process_command_results(
                results,
                &mut ctx,
                null_data,
                should_write_separator,
                unbuffered,
                binary,
                &mut suppress_autoprint,
                out,
            );

            match action {
                ProcessedResult::Done => break,
                ProcessedResult::Deleted => break 'script_cycle,
                ProcessedResult::Restart => continue 'script_cycle,
                ProcessedResult::Quit(code) => std::process::exit(code),
                ProcessedResult::NextLineAndResume(next_resume_pc) => {
                    if line_idx + 1 < all_lines.len() {
                        line_idx += 1;
                        ctx.set_current_line(line_idx + 1, all_lines_bytes[line_idx].clone());
                        ctx.set_filename(&all_filenames[line_idx]);
                        resume_from_pc = Some(next_resume_pc);
                        continue 'script_cycle;
                    } else {
                        break 'script_cycle;
                    }
                }
                ProcessedResult::AppendNextAndResume {
                    resume_pc,
                    pattern_space,
                } => {
                    let outcome = handle_n_command(
                        commands,
                        &mut ctx,
                        &mut evaluator,
                        all_lines,
                        all_lines_bytes,
                        all_filenames,
                        null_data,
                        posix,
                        unbuffered,
                        binary,
                        &mut suppress_autoprint,
                        &mut line_idx,
                        resume_pc,
                        pattern_space,
                        out,
                    )?;
                    match outcome {
                        NCommandOutcome::Complete => break,
                        NCommandOutcome::BreakCycle => break 'script_cycle,
                        NCommandOutcome::ContinueCycle => continue 'script_cycle,
                    }
                }
            }
        }
        line_idx += 1;
    }

    Ok(())
}
