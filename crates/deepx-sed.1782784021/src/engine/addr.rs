// Copyright (c) 2026 Red Authors
// License: MIT
//

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::errors::Result;

use crate::engine::pattern_space::PatternSpace;
use crate::engine::types::SedRegex;
use crate::parser::Address;
use crate::parser::AddressRange;
use crate::util::regex::compile_regex;

#[derive(Debug)]
pub struct ExecutionContext {
    pub current_line_num: usize,
    pub total_lines: Option<usize>,
    pub pattern_space: PatternSpace,
    pub quiet_mode: bool,
    pub hold_space: String,
    pub hold_space_raw: Vec<u8>, // Raw bytes for hold space (preserves invalid UTF-8)
    pub current_filename: String,
    pub r_file_handles: HashMap<String, BufReader<File>>, // For R command: track open files
    pub extended_regex: bool,                             // -r/-E flag for ERE mode
    pub line_length: usize,       // -l N flag for line wrapping in 'l' command
    pub unbuffered: bool,         // -u flag for unbuffered output
    pub null_data: bool,          // -z flag for NUL-separated lines
    pub all_input_consumed: bool, // True if all input lines have been read (via N reaching EOF)
}

impl ExecutionContext {
    pub fn new(
        total_lines: Option<usize>,
        quiet_mode: bool,
        extended_regex: bool,
        line_length: usize,
        unbuffered: bool,
        null_data: bool,
    ) -> Self {
        Self {
            current_line_num: 0,
            total_lines,
            pattern_space: PatternSpace::default(),
            quiet_mode,
            hold_space: String::new(),
            hold_space_raw: Vec::new(),
            current_filename: "-".to_string(), // Default to stdin
            r_file_handles: HashMap::new(),
            extended_regex,
            line_length,
            unbuffered,
            null_data,
            all_input_consumed: false,
        }
    }

    pub fn set_filename(&mut self, filename: &str) {
        self.current_filename = filename.to_string();
    }

    /// Read one line from file for R command. Returns None if EOF or error.
    pub fn read_line_from_file(&mut self, path: &str) -> Option<String> {
        // Get or create file handle for this path
        if !self.r_file_handles.contains_key(path) {
            // Try to open the file
            match File::open(path) {
                Ok(file) => {
                    self.r_file_handles
                        .insert(path.to_string(), BufReader::new(file));
                }
                Err(_) => {
                    // Silently ignore if file can't be opened
                    return None;
                }
            }
        }

        // Read one line from the file
        if let Some(reader) = self.r_file_handles.get_mut(path) {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => None, // EOF
                Ok(_) => {
                    // Remove trailing newline (will be added back when printing)
                    if line.ends_with('\n') {
                        line.pop();
                    }
                    Some(line)
                }
                Err(_) => None, // Error reading - silently ignore
            }
        } else {
            None
        }
    }

    pub fn set_current_line(&mut self, line_num: usize, raw: Vec<u8>) {
        self.current_line_num = line_num;
        // Use set_raw since raw is the source of truth
        // Text representation is derived on demand via PatternSpace
        self.pattern_space.set_raw(raw);
        // Reset flag - we're starting with a new line from input
        self.all_input_consumed = false;
    }
}

#[derive(Debug, Clone)]
struct RangeState {
    is_active: bool,
    active_until_line: Option<usize>,
}

impl RangeState {
    fn new() -> Self {
        Self {
            is_active: false,
            active_until_line: None,
        }
    }
}

#[derive(Debug)]
pub struct AddressEvaluator {
    regex_cache: HashMap<String, SedRegex>,
    /// Range states keyed by AddressRange.id (stable identifier)
    range_states: HashMap<u64, RangeState>,
    pub last_s_regex: Option<SedRegex>, // Unified "last regex" for s command and addresses
    extended_regex: bool,               // -r/-E flag
    posix: bool,                        // --posix flag
}

impl AddressEvaluator {
    pub fn new(extended_regex: bool, posix: bool) -> Self {
        Self {
            regex_cache: HashMap::new(),
            range_states: HashMap::new(),
            last_s_regex: None,
            extended_regex,
            posix,
        }
    }

    pub fn get_or_compile_regex(&mut self, pattern: &str) -> Result<SedRegex> {
        // Handle empty pattern - use unified last regex (from s command or address)
        if pattern.is_empty() {
            if let Some(ref regex) = self.last_s_regex {
                return Ok(regex.clone());
            } else {
                return Err(crate::errors::SedError::parse(
                    "no previous regular expression",
                ));
            }
        }

        // Compile and cache non-empty pattern
        if !self.regex_cache.contains_key(pattern) {
            let regex = compile_regex(
                pattern,
                self.extended_regex,
                false,
                false,
                false,
                self.posix,
                "address pattern",
            )?;
            self.regex_cache.insert(pattern.to_string(), regex.clone());
        }

        let regex = self.regex_cache.get(pattern).unwrap().clone();
        // Update unified last_s_regex (both s command and addresses share this)
        self.last_s_regex = Some(regex.clone());
        Ok(regex)
    }

    pub fn matches_address(&mut self, addr: &Address, ctx: &ExecutionContext) -> Result<bool> {
        match addr {
            Address::Line(n) => {
                // Line 0 has special meaning:
                // - In ranges like 0,/regexp/ it means "start from first line"
                // - As standalone it should never match (line numbers start at 1)
                if *n == 0 {
                    Ok(false) // Line 0 never matches by itself
                } else {
                    Ok(ctx.current_line_num == *n)
                }
            }
            Address::Dollar => {
                // $ matches if we're on the last line OR if all input has been consumed (via N)
                match ctx.total_lines {
                    Some(total) => Ok(ctx.current_line_num == total || ctx.all_input_consumed),
                    None => Ok(false),
                }
            }
            Address::Regex(pattern) => {
                let regex = self.get_or_compile_regex(pattern)?;
                Ok(regex.is_match(ctx.pattern_space.text()))
            }
            Address::Relative(base_addr, offset) => {
                let base_line = self.resolve_address_to_line(base_addr, ctx)?;
                match base_line {
                    Some(base) => {
                        Ok(ctx.current_line_num == (base as isize + offset).max(1) as usize)
                    }
                    None => Ok(false),
                }
            }
            Address::Step(base_addr, step) => {
                if *step == 0 {
                    return Ok(false);
                }
                let base_line = self.resolve_address_to_line(base_addr, ctx)?;
                match base_line {
                    Some(base) => {
                        if ctx.current_line_num >= base {
                            let diff = ctx.current_line_num - base;
                            Ok(diff % step == 0)
                        } else {
                            Ok(false)
                        }
                    }
                    None => Ok(false),
                }
            }
        }
    }

    pub fn resolve_address_to_line(
        &mut self,
        addr: &Address,
        ctx: &ExecutionContext,
    ) -> Result<Option<usize>> {
        match addr {
            Address::Line(n) => Ok(Some(*n)),
            Address::Dollar => match ctx.total_lines {
                Some(total) => Ok(Some(total)),
                None => Ok(None),
            },
            Address::Regex(_pattern) => Ok(None),
            Address::Relative(base_addr, offset) => {
                let base_line = self.resolve_address_to_line(base_addr, ctx)?;
                match base_line {
                    Some(base) => Ok(Some((base as isize + offset).max(1) as usize)),
                    None => Ok(None),
                }
            }
            Address::Step(_, _) => Ok(None),
        }
    }

    pub fn is_last_line(&self, ctx: &ExecutionContext) -> bool {
        matches!(ctx.total_lines, Some(total) if ctx.current_line_num == total)
    }
    pub fn is_first_line(&self, ctx: &ExecutionContext) -> bool {
        ctx.current_line_num == 1
    }
    pub fn is_valid_line_number(&self, line_num: usize, ctx: &ExecutionContext) -> bool {
        line_num > 0
            && match ctx.total_lines {
                Some(total) => line_num <= total,
                None => true,
            }
    }

    /// Evaluate range semantics specifically for the `c` (change) command.
    /// Returns (matches, is_end_of_range).
    ///
    /// Behaviour:
    /// - For single-address ranges (addr, or ,addr): match only on that line and mark as end.
    /// - For two-address ranges (addr1,addr2): match all lines from start until end; however,
    ///   `is_end_of_range` is true only on the line where the end is reached. This allows
    ///   the engine to print the replacement text once at the end of the range.
    pub fn evaluate_range_for_change(
        &mut self,
        range: &AddressRange,
        ctx: &ExecutionContext,
    ) -> Result<(bool, bool)> {
        match (&range.start, &range.end) {
            (Some(start_addr), Some(end_addr)) => {
                self.evaluate_two_address_range_for_change(range.id, start_addr, end_addr, ctx)
            }
            (Some(start_addr), None) => {
                let m = self.matches_address(start_addr, ctx)?;
                Ok((m, m))
            }
            (None, Some(end_addr)) => {
                let m = self.matches_address(end_addr, ctx)?;
                Ok((m, m))
            }
            (None, None) => Ok((true, true)),
        }
    }

    fn evaluate_two_address_range_for_change(
        &mut self,
        range_id: u64,
        start_addr: &Address,
        end_addr: &Address,
        ctx: &ExecutionContext,
    ) -> Result<(bool, bool)> {
        // Use range_id as stable key instead of pointer addresses
        let state = self
            .range_states
            .entry(range_id)
            .or_insert_with(RangeState::new);
        let (is_active, active_until_line) = (state.is_active, state.active_until_line);

        if !is_active {
            // Special handling for 0,addr2 range (GNU extension)
            // Line 0 means "activate from first line and check end immediately"
            let is_zero_range = matches!(start_addr, Address::Line(0));

            let should_activate = if is_zero_range {
                // For 0,addr2: activate on first line (line 1)
                ctx.current_line_num == 1
            } else {
                self.matches_address(start_addr, ctx)?
            };

            if should_activate {
                // Try to resolve a concrete end line for numeric-like ends
                let resolved_end: Option<usize> = match end_addr {
                    Address::Line(n) => Some(*n),
                    Address::Dollar => ctx.total_lines,
                    Address::Relative(base, offset) => match **base {
                        Address::Line(0) => {
                            Some((ctx.current_line_num as isize + *offset).max(1) as usize)
                        }
                        _ => self.resolve_address_to_line(end_addr, ctx)?,
                    },
                    _ => None,
                };

                // For REGEX end addresses, GNU sed checks the end pattern starting from
                // the NEXT line, not the same line. So /start/,/end/ where start==end
                // will match from the first 'start' until the NEXT 'end' (or EOF).
                // For numeric/relative addresses, same-line check is correct.
                let end_is_regex = matches!(end_addr, Address::Regex(_));
                // Check if end is +N relative offset (Relative with Line(0) base)
                // For +N offsets, we use resolved_end instead of matches_address
                // because +N means "N lines after range start", not "line N"
                let end_is_relative_offset = matches!(
                    end_addr,
                    Address::Relative(base, _) if matches!(**base, Address::Line(0))
                );

                if !end_is_regex && !end_is_relative_offset {
                    // For non-regex, non-relative ends, check if end matches on this line
                    let end_matches_same_line = self.matches_address(end_addr, ctx)?;
                    if end_matches_same_line {
                        // Single-line range: match and end here
                        let state = self
                            .range_states
                            .get_mut(&range_id)
                            .expect("state was inserted above");
                        state.is_active = false;
                        state.active_until_line = None;
                        return Ok((true, true));
                    }
                }

                if let Some(end_line) = resolved_end {
                    if end_line <= ctx.current_line_num {
                        // Degenerate range, end not in future -> end here
                        let state = self
                            .range_states
                            .get_mut(&range_id)
                            .expect("state was inserted above");
                        state.is_active = false;
                        state.active_until_line = None;
                        return Ok((true, true));
                    } else {
                        let state = self
                            .range_states
                            .get_mut(&range_id)
                            .expect("state was inserted above");
                        state.is_active = true;
                        state.active_until_line = Some(end_line);
                        return Ok((true, false));
                    }
                } else {
                    // Open-ended (regex) end; activate until end matches
                    let state = self
                        .range_states
                        .get_mut(&range_id)
                        .expect("state was inserted above");
                    state.is_active = true;
                    state.active_until_line = None;
                    return Ok((true, false));
                }
            }
        } else {
            // Inside active range
            let mut should_end_here = false;
            if let Some(end_line) = active_until_line {
                if ctx.current_line_num >= end_line {
                    should_end_here = true;
                }
            } else if self.matches_address(end_addr, ctx)? {
                should_end_here = true;
            }
            if should_end_here {
                let state = self
                    .range_states
                    .get_mut(&range_id)
                    .expect("state was inserted above");
                state.is_active = false;
                state.active_until_line = None;
                return Ok((true, true));
            }
            return Ok((true, false));
        }

        Ok((false, false))
    }

    pub fn matches_range(&mut self, range: &AddressRange, ctx: &ExecutionContext) -> Result<bool> {
        match (&range.start, &range.end) {
            (Some(start_addr), None) => self.matches_address(start_addr, ctx),
            (Some(start_addr), Some(end_addr)) => {
                self.evaluate_two_address_range(range.id, start_addr, end_addr, ctx)
            }
            (None, Some(end_addr)) => self.matches_address(end_addr, ctx),
            (None, None) => Ok(true),
        }
    }

    fn evaluate_two_address_range(
        &mut self,
        range_id: u64,
        start_addr: &Address,
        end_addr: &Address,
        ctx: &ExecutionContext,
    ) -> Result<bool> {
        // Use range_id as stable key instead of pointer addresses
        let state = self
            .range_states
            .entry(range_id)
            .or_insert_with(RangeState::new);
        let (is_active, active_until_line) = (state.is_active, state.active_until_line);

        if !is_active {
            // Special handling for 0,addr2 range (GNU extension)
            // Line 0 means "activate from first line and check end immediately"
            let is_zero_range = matches!(start_addr, Address::Line(0));

            let should_activate = if is_zero_range {
                // For 0,addr2: activate on first line (line 1)
                ctx.current_line_num == 1
            } else {
                self.matches_address(start_addr, ctx)?
            };

            if should_activate {
                let resolved_end: Option<usize> = match end_addr {
                    Address::Line(n) => Some(*n),
                    Address::Dollar => ctx.total_lines,
                    Address::Relative(base, offset) => match **base {
                        Address::Line(0) => {
                            Some((ctx.current_line_num as isize + *offset).max(1) as usize)
                        }
                        _ => self.resolve_address_to_line(end_addr, ctx)?,
                    },
                    _ => None,
                };

                // For regex end addresses: GNU sed checks end pattern from NEXT line
                // (after the line that matched start). The only exception is 0,/regexp/
                // which checks regexp on line 1.
                // For +N relative offsets, we use resolved_end instead of matches_address
                // because +N means "N lines after range start", not "line N".
                let end_is_relative_offset = matches!(
                    end_addr,
                    Address::Relative(base, _) if matches!(**base, Address::Line(0))
                );
                let end_matches_same_line = if matches!(end_addr, Address::Regex(_)) {
                    if is_zero_range {
                        // For 0,/regexp/: check regexp on line 1
                        self.matches_address(end_addr, ctx)?
                    } else {
                        // For all other /start/,/end/ or N,/end/: check end from NEXT line
                        false
                    }
                } else if end_is_relative_offset {
                    // For +N offsets, don't use matches_address - use resolved_end instead
                    false
                } else {
                    self.matches_address(end_addr, ctx)?
                };

                let state = self
                    .range_states
                    .get_mut(&range_id)
                    .expect("state was inserted above");
                if end_matches_same_line {
                    state.is_active = false;
                    state.active_until_line = None;
                } else if let Some(end_line) = resolved_end {
                    if end_line <= ctx.current_line_num {
                        state.is_active = false;
                        state.active_until_line = None;
                    } else {
                        state.is_active = true;
                        state.active_until_line = Some(end_line);
                    }
                } else {
                    state.is_active = true;
                    state.active_until_line = None;
                }
                return Ok(true);
            }
        } else {
            if let Some(end_line) = active_until_line {
                if ctx.current_line_num < end_line {
                    // Still inside the active range
                    return Ok(true);
                } else if ctx.current_line_num == end_line {
                    // Last line of the range: deactivate after matching
                    let state = self
                        .range_states
                        .get_mut(&range_id)
                        .expect("state was inserted above");
                    state.is_active = false;
                    state.active_until_line = None;
                    return Ok(true);
                } else {
                    // Past the end of the range: ensure deactivated and no match
                    let state = self
                        .range_states
                        .get_mut(&range_id)
                        .expect("state was inserted above");
                    state.is_active = false;
                    state.active_until_line = None;
                    return Ok(false);
                }
            } else {
                // Open-ended (regex) end: if end matches now, include this line and deactivate
                if self.matches_address(end_addr, ctx)? {
                    let state = self
                        .range_states
                        .get_mut(&range_id)
                        .expect("state was inserted above");
                    state.is_active = false;
                    state.active_until_line = None;
                    return Ok(true);
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn evaluate_with_negation(
        &mut self,
        range: Option<&AddressRange>,
        negated: bool,
        ctx: &ExecutionContext,
    ) -> Result<bool> {
        let matches = match range {
            Some(r) => self.matches_range(r, ctx)?,
            None => true,
        };
        Ok(if negated { !matches } else { matches })
    }
}
