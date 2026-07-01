//! Shared grammar helpers: identifiers, quoted strings, and numbers.

use serde_json::Value;

use crate::lexer::TokenKind;

use super::stream::Stream;

/// Convert numeric token text to a JSON number with JavaScript `Number`
/// semantics. Integral values serialize without a fractional part.
pub fn js_number(text: &str) -> Value {
    let f: f64 = text.parse().unwrap_or(f64::NAN);
    number_from_f64(f)
}

/// Build a JSON number from an f64 the way `JSON.stringify` renders a JS number.
///
/// Whole values become integers. Values outside the i64 or u64 range but still
/// whole become an integer-valued float, which serde renders without a decimal
/// only when it fits an integer type, so large whole numbers fall back to the
/// float path. In practice the DDL numbers stay within i64.
pub fn number_from_f64(f: f64) -> Value {
    if f.is_finite() && f.fract() == 0.0 {
        if f >= i64::MIN as f64 && f <= i64::MAX as f64 {
            return Value::from(f as i64);
        }
        if f >= 0.0 && f <= u64::MAX as f64 {
            return Value::from(f as u64);
        }
    }
    serde_json::Number::from_f64(f)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// Parse `S_IDENTIFIER`: a quoted identifier, an unquoted identifier, or any
/// keyword used as an identifier. Returns the decoded value.
pub fn s_identifier(s: &mut Stream) -> Option<String> {
    match s.peek()?.kind.clone() {
        TokenKind::IdentifierQuoted => s.eat(&TokenKind::IdentifierQuoted),
        TokenKind::IdentifierUnquoted => s.eat(&TokenKind::IdentifierUnquoted),
        TokenKind::Keyword(_) => {
            // Any keyword may stand in for an identifier. Yield its raw value.
            let v = s.peek().map(|t| t.value.clone());
            s.bump();
            v
        }
        _ => None,
    }
}

/// Parse `O_QUOTED_STRING`: a double- or single-quoted string value.
pub fn o_quoted_string(s: &mut Stream) -> Option<String> {
    s.eat(&TokenKind::DQuoteString)
        .or_else(|| s.eat(&TokenKind::SQuoteString))
}

/// Parse `O_CHARSET`, `O_COLLATION`, `O_ENGINE`: a quoted string or identifier.
pub fn o_string_or_ident(s: &mut Stream) -> Option<String> {
    if let Some(v) = o_quoted_string(s) {
        return Some(v);
    }
    s_identifier(s)
}

/// Parse `O_TABLE_OPTION_VALUE`: a quoted string, identifier, or number.
///
/// A number is returned as a JSON value to keep numeric options numeric.
pub fn o_table_option_value(s: &mut Stream) -> Option<Value> {
    if let Some(v) = o_quoted_string(s) {
        return Some(Value::String(v));
    }
    let save = s.pos();
    if let Some(v) = s_identifier(s) {
        return Some(Value::String(v));
    }
    s.set(save);
    if let Some(text) = s.eat(&TokenKind::Number) {
        return Some(js_number(&text));
    }
    None
}

/// Parse the value of `S_NUMBER` as a JSON number.
pub fn s_number(s: &mut Stream) -> Option<Value> {
    s.eat(&TokenKind::Number).map(|t| js_number(&t))
}
