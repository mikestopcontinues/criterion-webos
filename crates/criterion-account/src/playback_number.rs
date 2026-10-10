//! Composed native floating-literal grammar from frozen contract 42a5cade and
//! addendum 15cc49b2. The permissive lexical screen alone is insufficient.
//! This is a pure parser; it does not execute or certify the native runtime.

/// Conservative application limit on original UTF-8 bytes, before trimming.
pub(crate) const MAX_PRIMITIVE: usize = 16 * 1024;

pub(crate) fn parse(content: &str) -> Option<f64> {
    if content.len() > MAX_PRIMITIVE {
        return None;
    }
    let content = content.trim_matches(|ch: char| ch <= '\u{20}');
    let (negative, magnitude) = match content.as_bytes().first()? {
        b'-' => (true, &content[1..]),
        b'+' => (false, &content[1..]),
        _ => (false, content),
    };
    match magnitude {
        "NaN" => return Some(f64::NAN),
        "Infinity" => {
            return Some(if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            });
        }
        _ => {}
    }
    let magnitude = match magnitude.as_bytes().last()? {
        b'f' | b'F' | b'd' | b'D' => &magnitude[..magnitude.len() - 1],
        _ => magnitude,
    };
    let bytes = magnitude.as_bytes();
    let value = if bytes.starts_with(b"0x") || bytes.starts_with(b"0X") {
        hex(bytes)?
    } else {
        if !decimal(bytes) {
            return None;
        }
        magnitude.parse::<f64>().ok()?
    };
    Some(if negative { -value } else { value })
}

fn decimal(bytes: &[u8]) -> bool {
    let mut cursor = 0;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    let mut digits = cursor;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        digits += cursor - start;
    }
    if digits == 0 {
        return false;
    }
    if matches!(bytes.get(cursor), Some(b'e' | b'E')) {
        cursor += 1;
        if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
            cursor += 1;
        }
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == start {
            return false;
        }
    }
    cursor == bytes.len()
}

fn hex(bytes: &[u8]) -> Option<f64> {
    let mut cursor = 2;
    let mut point = false;
    let mut digits = 0;
    let mut fractional_digits = 0;
    // First 54 significant bits: the normal 53-bit significand and its guard.
    // Remaining nonzero bits only need one sticky flag; no mantissa allocation.
    let mut head = 0_u64;
    let mut bit_length = 0_usize;
    let mut sticky = false;
    loop {
        let byte = *bytes.get(cursor)?;
        let nibble = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            b'.' if !point => {
                point = true;
                cursor += 1;
                continue;
            }
            b'p' | b'P' if digits > 0 => break,
            _ => return None,
        };
        digits += 1;
        fractional_digits += usize::from(point);
        for shift in (0..4).rev() {
            let bit = (nibble >> shift) & 1;
            if bit_length != 0 || bit != 0 {
                bit_length += 1;
                if bit_length <= 54 {
                    head = (head << 1) | u64::from(bit);
                } else {
                    sticky |= bit != 0;
                }
            }
        }
        cursor += 1;
    }
    cursor += 1;
    let negative_exponent = bytes.get(cursor) == Some(&b'-');
    if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
        cursor += 1;
    }
    let start = cursor;
    let mut exponent = 0_i64;
    while let Some(byte @ b'0'..=b'9') = bytes.get(cursor) {
        exponent = exponent.saturating_mul(10);
        let digit = i64::from(byte - b'0');
        exponent = if negative_exponent {
            exponent.saturating_sub(digit)
        } else {
            exponent.saturating_add(digit)
        };
        cursor += 1;
    }
    if cursor == start || cursor != bytes.len() {
        return None;
    }
    if bit_length == 0 {
        return Some(0.0);
    }
    // The 16 KiB input cap bounds mantissa adjustments below 65536 bits.
    // Exponents saturated at i64 extremes cannot cancel into binary64 range.
    exponent = exponent
        .saturating_sub((fractional_digits * 4) as i64)
        .saturating_add((bit_length - 1) as i64);
    head <<= 54 - bit_length.min(54);
    Some(round_hex(head, sticky, exponent))
}

fn round_hex(head: u64, sticky: bool, mut exponent: i64) -> f64 {
    if exponent > 1023 {
        return f64::INFINITY;
    }
    if exponent < -1075 {
        return 0.0;
    }
    let encoded = if exponent >= -1022 {
        let mut significand = head >> 1;
        if head & 1 != 0 && (sticky || significand & 1 != 0) {
            significand += 1;
        }
        if significand == 1 << 53 {
            significand >>= 1;
            exponent += 1;
        }
        if exponent > 1023 {
            return f64::INFINITY;
        }
        ((exponent + 1023) as u64) << 52 | (significand & ((1 << 52) - 1))
    } else {
        // Keep 0..52 bits at the fixed subnormal quantum 2^-1074. Round
        // directly here; rounding first to a normal significand would double-round.
        let shift = 54 - (exponent + 1075) as u32;
        let mut significand = head >> shift;
        let guard = head >> (shift - 1) & 1 != 0;
        let tail = sticky || head & ((1 << (shift - 1)) - 1) != 0;
        if guard && (tail || significand & 1 != 0) {
            significand += 1;
        }
        significand
    };
    f64::from_bits(encoded)
}

#[cfg(test)]
mod tests {
    use super::{MAX_PRIMITIVE, parse};

    fn bits(content: &str, expected: u64) {
        assert_eq!(
            parse(content).map(f64::to_bits),
            Some(expected),
            "{content:?}"
        );
    }

    #[test]
    fn decimal_suffix_still_rounds_to_binary64_after_ascii_trim_and_sign() {
        // 12.5 = binary1100.1; exponent3, fraction0x9000000000000.
        bits("\0\t +12.5F\r\n", 0x4029_0000_0000_0000);
    }

    #[test]
    fn decimal_literals_keep_binary64_rounding_and_i32_threshold_values() {
        // Literal IEEE-754 encodings; none use this parser to build the oracle.
        for (content, expected) in [
            ("16777217f", 0x4170_0000_1000_0000),
            ("9007199254740993D", 0x4340_0000_0000_0000),
            ("9007199254740995D", 0x4340_0000_0000_0002),
            (".5E+1d", 0x4014_0000_0000_0000),
            ("1.e-1", 0x3fb9_9999_9999_999a),
            ("2147483647", 0x41df_ffff_ffc0_0000),
            ("2147483647.5", 0x41df_ffff_ffe0_0000),
            ("2147483648.0", 0x41e0_0000_0000_0000),
            ("-2147483648", 0xc1e0_0000_0000_0000),
            ("1e99999999999999999999999", 0x7ff0_0000_0000_0000),
            ("-1e99999999999999999999999", 0xfff0_0000_0000_0000),
            ("1e-99999999999999999999999", 0),
            ("-1e-99999999999999999999999", 0x8000_0000_0000_0000),
            ("-0e99999999999999999999999", 0x8000_0000_0000_0000),
        ] {
            bits(content, expected);
        }
    }

    #[test]
    fn special_tokens_are_exact_and_signs_do_not_change_nan() {
        for content in ["NaN", "+NaN", "-NaN", "\u{1f} -NaN \0"] {
            bits(content, 0x7ff8_0000_0000_0000);
        }
        bits("+Infinity", 0x7ff0_0000_0000_0000);
        bits("\0 -Infinity\t", 0xfff0_0000_0000_0000);
        for content in [
            "nan",
            "NAN",
            "inf",
            "INF",
            "NaNd",
            "Infinityf",
            "+InfinityD",
            ".NaN",
        ] {
            assert!(parse(content).is_none(), "{content:?}");
        }
    }

    #[test]
    fn primitive_cap_is_inclusive_and_applies_before_trimming() {
        let boundary = format!("0{}", " ".repeat(MAX_PRIMITIVE - 1));
        bits(&boundary, 0);
        assert!(parse(&(boundary + " ")).is_none());
    }

    #[test]
    fn hex_normal_literals_round_once_with_guard_sticky_and_even_ties() {
        for (content, expected) in [
            ("0x1.00000000000008p0", 0x3ff0_0000_0000_0000),
            (
                "0x1.00000000000008000000000000000001p0",
                0x3ff0_0000_0000_0001,
            ),
            ("0x1.00000000000018p0", 0x3ff0_0000_0000_0002),
            ("0x1.fffffffffffff8p0", 0x4000_0000_0000_0000),
            ("0x1.00000000000007fffffffffffffffp0", 0x3ff0_0000_0000_0000),
            ("0x0.0000000000001p0", 0x3cb0_0000_0000_0000),
            ("\0 +0XAb.CP+4F\r", 0x40a5_7800_0000_0000),
        ] {
            bits(content, expected);
        }
    }

    #[test]
    fn hex_subnormal_midpoints_and_normal_boundary_use_one_rounding() {
        for (content, expected) in [
            ("0x1p-1074", 1),
            ("0x1p-1075", 0),
            ("0x1.0000000000001p-1075", 1),
            ("0x1.8p-1074", 2),
            ("0x1.4p-1074", 1),
            ("0x1.fffffffffffffp-1023", 0x0010_0000_0000_0000),
            ("0x1.ffffffffffffefffp-1023", 0x000f_ffff_ffff_ffff),
            ("0x0.fffffffffffffp-1022", 0x000f_ffff_ffff_ffff),
            ("0x1p-1022", 0x0010_0000_0000_0000),
            ("0x1.00000000000008p-1022", 0x0010_0000_0000_0000),
            ("0x1.00000000000018p-1022", 0x0010_0000_0000_0002),
            ("-0x1p-1075", 0x8000_0000_0000_0000),
        ] {
            bits(content, expected);
        }
    }

    #[test]
    fn overflow_underflow_and_huge_exponents_preserve_sign_and_zero() {
        for (content, expected) in [
            ("1.7976931348623157e308", 0x7fef_ffff_ffff_ffff),
            ("1.7976931348623159e308", 0x7ff0_0000_0000_0000),
            ("2.4703282292062327e-324", 0),
            ("2.4703282292062328e-324", 1),
            ("4.9406564584124654e-324", 1),
            ("0x1.fffffffffffffp1023", 0x7fef_ffff_ffff_ffff),
            ("0x1.fffffffffffff7ffffffffp1023", 0x7fef_ffff_ffff_ffff),
            ("0x1.fffffffffffff8p1023", 0x7ff0_0000_0000_0000),
            ("-0x1p1024d", 0xfff0_0000_0000_0000),
            ("-0x1.fffffffffffffp-1076F", 0x8000_0000_0000_0000),
            ("0x1p99999999999999999999999999", 0x7ff0_0000_0000_0000),
            ("-0x1p-99999999999999999999999999", 0x8000_0000_0000_0000),
            ("0x0p99999999999999999999999999", 0),
            (
                "-0x0.000p+99999999999999999999999999",
                0x8000_0000_0000_0000,
            ),
            ("0x7fffffffp0", 0x41df_ffff_ffc0_0000),
            ("0x7fffffff.8p0", 0x41df_ffff_ffe0_0000),
            ("-0x80000000p0", 0xc1e0_0000_0000_0000),
        ] {
            bits(content, expected);
        }
    }

    #[test]
    fn large_mantissa_exponent_cancellation_has_no_intermediate_float() {
        bits(
            &format!("0x.{}1p4004", "0".repeat(1000)),
            0x3ff0_0000_0000_0000,
        );
        bits(
            &format!("0x1{}p-4000", "0".repeat(1000)),
            0x3ff0_0000_0000_0000,
        );
        // A far tail bit must survive as sticky above the exact even midpoint.
        bits(
            &format!("0x1.00000000000008{}1p0", "0".repeat(1000)),
            0x3ff0_0000_0000_0001,
        );
    }

    #[test]
    fn composed_grammar_rejects_screen_only_and_malformed_candidates() {
        for content in [
            "",
            "\0 \t",
            "+",
            "-",
            "1ef",
            "1e",
            "1e+",
            "1.2.3",
            "0x1",
            "0x1.",
            "0x.p0",
            "0x1p",
            "0x1p+",
            "0x1p1.0",
            "0x1p0ff",
            "0x1e0",
            "--1",
            "-+1",
            "1 2",
            "1_2",
            "0b1",
            "0x1p0\u{a0}",
            "\u{a0}1",
            "1\n2",
            "0x1p0fD",
            "0x1p0dfoo",
            "0x1p 0",
            "０.5",
            "0xＦp0",
            "0x1..0p0",
            "0x1p--1",
            "1dF",
            "\u{21}1",
            "1\0f",
            "0x0p-99999999999999999999999e",
        ] {
            assert!(parse(content).is_none(), "{content:?}");
        }
        for (content, expected) in [
            ("0x.8p1", 0x3ff0_0000_0000_0000),
            ("0x1.p0D", 0x3ff0_0000_0000_0000),
            ("+01.f", 0x3ff0_0000_0000_0000),
            ("-0.F", 0x8000_0000_0000_0000),
            ("\u{1}0x1p0\u{20}", 0x3ff0_0000_0000_0000),
        ] {
            bits(content, expected);
        }
    }
}
