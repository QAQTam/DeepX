//! Token counting: CJK heuristic + breakdown types.

// ── Heuristic-only counting (no tokenizer dependency) ──

/// Count tokens using a CJK-aware character heuristic.
///
/// Non-CJK characters are counted as `len / 3.3`; CJK characters as
/// `count / 1.67`.  These ratios are derived from empirical DeepSeek
/// tokenizer measurements and serve as a dependency-free fallback when
/// no `tokenizer.json` is available.
pub fn count_tokens(text: &str) -> u32 {
    let cjk: usize = text
        .chars()
        .filter(|c| {
            matches!(
                c,
                '\u{4e00}'..='\u{9fff}'
                    | '\u{3400}'..='\u{4dbf}'
                    | '\u{3000}'..='\u{303f}'
                    | '\u{ff00}'..='\u{ffef}'
                    | '\u{3040}'..='\u{30ff}'
            )
        })
        .count();
    (text.len().saturating_sub(cjk) as f64 / 3.3 + cjk as f64 / 1.67) as u32
}

// ── Breakdown ──

/// Token usage broken down by category.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokenBreakdown {
    pub system: u32,
    pub episodic: u32,
    pub total: u32,
}
