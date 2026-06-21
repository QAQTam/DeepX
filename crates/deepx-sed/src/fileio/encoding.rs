// Copyright (c) 2026 Red Authors
// License: MIT
//

//! Encoding detection utilities

use encoding_rs::Encoding;

/// Detect the encoding of a byte sequence
/// Returns the detected encoding based on BOM, UTF-8 validity, or locale settings
pub fn detect_encoding(bytes: &[u8]) -> &'static Encoding {
    // Try to detect from BOM (Byte Order Mark)
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return encoding_rs::UTF_8;
    }

    // Try UTF-8 validation
    if std::str::from_utf8(bytes).is_ok() {
        return encoding_rs::UTF_8;
    }

    // Check environment variables for locale
    // Try LC_ALL first, then LC_CTYPE, then LANG
    let locale = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_CTYPE"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();

    let locale_lower = locale.to_lowercase();

    // Match common encodings
    if locale_lower.contains("utf-8") || locale_lower.contains("utf8") {
        return encoding_rs::UTF_8;
    }
    if locale_lower.contains("euc-jp") || locale_lower.contains("eucjp") {
        return encoding_rs::EUC_JP;
    }
    if locale_lower.contains("shift_jis") || locale_lower.contains("sjis") {
        return encoding_rs::SHIFT_JIS;
    }
    if locale_lower.contains("iso-2022-jp") || locale_lower.contains("iso2022jp") {
        return encoding_rs::ISO_2022_JP;
    }
    if locale_lower.contains("iso-8859-1")
        || locale_lower.contains("iso88591")
        || locale_lower.contains("latin1")
    {
        return encoding_rs::WINDOWS_1252; // Close approximation to ISO-8859-1
    }
    if locale_lower.contains("windows-1252") || locale_lower.contains("cp1252") {
        return encoding_rs::WINDOWS_1252;
    }
    if locale_lower.contains("gb")
        || locale_lower.contains("gbk")
        || locale_lower.contains("gb2312")
    {
        return encoding_rs::GBK;
    }
    if locale_lower.contains("big5") {
        return encoding_rs::BIG5;
    }
    if locale_lower.contains("euc-kr") || locale_lower.contains("euckr") {
        return encoding_rs::EUC_KR;
    }

    // Default to UTF-8
    encoding_rs::UTF_8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf8_bom() {
        let bytes = &[0xEF, 0xBB, 0xBF, b'h', b'e', b'l', b'l', b'o'];
        assert_eq!(detect_encoding(bytes), encoding_rs::UTF_8);
    }

    #[test]
    fn test_valid_utf8() {
        let bytes = b"hello world";
        assert_eq!(detect_encoding(bytes), encoding_rs::UTF_8);
    }

    #[test]
    fn test_empty_bytes() {
        let bytes: &[u8] = &[];
        assert_eq!(detect_encoding(bytes), encoding_rs::UTF_8);
    }
}
