//! Tokenizer for MySQL and MariaDB DDL.
//!
//! Splits a statement into tokens the grammar consumes. Rules run in
//! declaration order and the first match at the current position wins, the way
//! a compiled regex alternation picks the first matching alternative. Keywords
//! come first (longest declared first), then symbols and literals, then a
//! wildcard fallback.

use crate::keywords::KEYWORDS;

/// Token kinds produced by the lexer.
///
/// Keyword tokens carry the name in uppercase (for example `K_CREATE`). Symbol
/// tokens carry a fixed kind. Each token also keeps its decoded value, which is
/// the text a grammar rule reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// Whitespace or a SQL comment. Skipped by the grammar helpers.
    Ws,
    Equal,
    LParens,
    RParens,
    Comma,
    Semicolon,
    /// A bit literal such as `b'0101'` or `0b0101`.
    BitFormat,
    /// A hexadecimal literal such as `X'1F'` or `0x1f`.
    HexaFormat,
    /// A double-quoted string.
    DQuoteString,
    /// A single-quoted string.
    SQuoteString,
    /// A numeric literal.
    Number,
    /// A backtick-quoted identifier.
    IdentifierQuoted,
    /// An unquoted identifier.
    IdentifierUnquoted,
    /// A keyword. The stored `String` is the keyword name in uppercase.
    Keyword(String),
    /// Fallback for any other single run of characters.
    Unknown,
}

/// A lexed token.
#[derive(Debug, Clone)]
pub struct Token {
    /// The token kind.
    pub kind: TokenKind,
    /// The decoded value. For strings and identifiers this is the unescaped
    /// text. For numbers this is the raw matched text (parsed later). For
    /// keywords this is the raw matched text with original case.
    pub value: String,
}

/// A lexing error at a byte offset with the remaining input.
#[derive(Debug, Clone)]
pub struct LexError {
    /// Line number where lexing stopped, counting from 1.
    pub line: usize,
}

/// Tokenize a whole statement.
///
/// Returns all tokens including whitespace tokens. Grammar rules skip
/// whitespace where the source grammar allows it.
pub fn tokenize(input: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0usize;
    let mut tokens = Vec::new();

    while pos < chars.len() {
        match next_token(&chars, pos) {
            Some((token, len)) => {
                pos += len;
                tokens.push(token);
            }
            None => {
                // Nothing matched. Report the line number of the failure.
                let line = 1 + chars[..pos].iter().filter(|&&c| c == '\n').count();
                return Err(LexError { line });
            }
        }
    }

    Ok(tokens)
}

/// Try to match a single token at `start`. Returns the token and the number of
/// characters consumed.
///
/// The compiled lexer joins all rules into one alternation. A regex alternation
/// takes the first alternative that matches at the position, so the rules run
/// in declaration order and the first match wins. Keywords come first (longest
/// declared first), then symbols and literals, then the `.+` fallback.
fn next_token(chars: &[char], start: usize) -> Option<(Token, usize)> {
    // Keywords first, in declaration order.
    for (name, letters) in KEYWORDS.iter() {
        if let Some(len) = match_keyword(chars, start, letters) {
            let value: String = chars[start..start + len].iter().collect();
            return Some((
                Token {
                    kind: TokenKind::Keyword((*name).to_string()),
                    value,
                },
                len,
            ));
        }
    }

    // Symbols and literals, in declaration order.
    if let Some(len) = match_ws(chars, start) {
        return Some((tok(TokenKind::Ws, chars, start, len), len));
    }
    if chars[start] == '=' {
        return Some((tok(TokenKind::Equal, chars, start, 1), 1));
    }
    if chars[start] == '(' {
        return Some((tok(TokenKind::LParens, chars, start, 1), 1));
    }
    if chars[start] == ')' {
        return Some((tok(TokenKind::RParens, chars, start, 1), 1));
    }
    if chars[start] == ',' {
        return Some((tok(TokenKind::Comma, chars, start, 1), 1));
    }
    if chars[start] == ';' {
        return Some((tok(TokenKind::Semicolon, chars, start, 1), 1));
    }
    if let Some(len) = match_bit(chars, start) {
        return Some((tok(TokenKind::BitFormat, chars, start, len), len));
    }
    if let Some(len) = match_hexa(chars, start) {
        return Some((tok(TokenKind::HexaFormat, chars, start, len), len));
    }
    if let Some((len, value)) = match_dquote(chars, start) {
        return Some((
            Token {
                kind: TokenKind::DQuoteString,
                value,
            },
            len,
        ));
    }
    if let Some((len, value)) = match_squote(chars, start) {
        return Some((
            Token {
                kind: TokenKind::SQuoteString,
                value,
            },
            len,
        ));
    }
    if let Some(len) = match_number(chars, start) {
        return Some((tok(TokenKind::Number, chars, start, len), len));
    }
    if let Some((len, value)) = match_ident_quoted(chars, start) {
        return Some((
            Token {
                kind: TokenKind::IdentifierQuoted,
                value,
            },
            len,
        ));
    }
    if let Some(len) = match_ident_unquoted(chars, start) {
        return Some((tok(TokenKind::IdentifierUnquoted, chars, start, len), len));
    }
    // Fallback wildcard: `.+` matches the rest of the line greedily. It stops at
    // a line break because `.` does not match newlines by default.
    let mut end = start;
    while end < chars.len() && chars[end] != '\n' && chars[end] != '\r' {
        end += 1;
    }
    let unknown_len = (end - start).max(1);
    Some((
        tok(TokenKind::Unknown, chars, start, unknown_len),
        unknown_len,
    ))
}

/// Build a token whose value is the raw matched text.
fn tok(kind: TokenKind, chars: &[char], start: usize, len: usize) -> Token {
    Token {
        kind,
        value: chars[start..start + len].iter().collect(),
    }
}

/// Match a keyword with word boundaries and case-insensitive letters.
///
/// The `letters` slice holds each expected character. A boundary before and
/// after the match is required, matching `\b...\b`.
fn match_keyword(chars: &[char], start: usize, letters: &[char]) -> Option<usize> {
    let n = letters.len();
    if start + n > chars.len() {
        return None;
    }
    for (i, want) in letters.iter().enumerate() {
        let got = chars[start + i];
        if !got.eq_ignore_ascii_case(want) {
            return None;
        }
    }
    // Boundary check. `\b` sits between a word char and a non-word char.
    if start > 0 && is_word(chars[start - 1]) && is_word(letters[0]) {
        return None;
    }
    let after = start + n;
    if after < chars.len() && is_word(chars[after]) && is_word(letters[n - 1]) {
        return None;
    }
    Some(n)
}

/// Whether a character counts as a word character for `\b`.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Match whitespace and SQL comments: runs of `\s`, `#...`, `-- +...`, `/* ... */`.
fn match_ws(chars: &[char], start: usize) -> Option<usize> {
    let mut pos = start;
    loop {
        let before = pos;
        // Plain whitespace run.
        while pos < chars.len() && chars[pos].is_whitespace() {
            pos += 1;
        }
        // `#` to end of line.
        if pos < chars.len() && chars[pos] == '#' {
            while pos < chars.len() && chars[pos] != '\n' && chars[pos] != '\r' {
                pos += 1;
            }
        }
        // `-- ` then rest of line. Requires dash-dash then one or more spaces.
        if pos + 2 < chars.len()
            && chars[pos] == '-'
            && chars[pos + 1] == '-'
            && chars[pos + 2] == ' '
        {
            pos += 2;
            while pos < chars.len() && chars[pos] == ' ' {
                pos += 1;
            }
            while pos < chars.len() && chars[pos] != '\n' && chars[pos] != '\r' {
                pos += 1;
            }
        }
        // `/* ... */` block comment.
        if pos + 1 < chars.len() && chars[pos] == '/' && chars[pos + 1] == '*' {
            let mut end = pos + 2;
            while end + 1 < chars.len() && !(chars[end] == '*' && chars[end + 1] == '/') {
                end += 1;
            }
            if end + 1 < chars.len() {
                pos = end + 2;
            }
        }
        if pos == before {
            break;
        }
    }
    if pos > start {
        Some(pos - start)
    } else {
        None
    }
}

/// Match a bit literal.
fn match_bit(chars: &[char], start: usize) -> Option<usize> {
    // b'[01]+'
    if chars[start] == 'b' && start + 1 < chars.len() && chars[start + 1] == '\'' {
        let mut pos = start + 2;
        let digits_start = pos;
        while pos < chars.len() && (chars[pos] == '0' || chars[pos] == '1') {
            pos += 1;
        }
        if pos > digits_start && pos < chars.len() && chars[pos] == '\'' {
            return Some(pos + 1 - start);
        }
    }
    // 0b[01]+
    if chars[start] == '0' && start + 1 < chars.len() && chars[start + 1] == 'b' {
        let mut pos = start + 2;
        let digits_start = pos;
        while pos < chars.len() && (chars[pos] == '0' || chars[pos] == '1') {
            pos += 1;
        }
        if pos > digits_start {
            return Some(pos - start);
        }
    }
    None
}

/// Match a hexadecimal literal.
fn match_hexa(chars: &[char], start: usize) -> Option<usize> {
    // [Xx]'[0-9a-fA-F]+'
    if (chars[start] == 'X' || chars[start] == 'x')
        && start + 1 < chars.len()
        && chars[start + 1] == '\''
    {
        let mut pos = start + 2;
        let digits_start = pos;
        while pos < chars.len() && chars[pos].is_ascii_hexdigit() {
            pos += 1;
        }
        if pos > digits_start && pos < chars.len() && chars[pos] == '\'' {
            return Some(pos + 1 - start);
        }
    }
    // 0x[0-9a-fA-F]+
    if chars[start] == '0' && start + 1 < chars.len() && chars[start + 1] == 'x' {
        let mut pos = start + 2;
        let digits_start = pos;
        while pos < chars.len() && chars[pos].is_ascii_hexdigit() {
            pos += 1;
        }
        if pos > digits_start {
            return Some(pos - start);
        }
    }
    None
}

/// Match a double-quoted string and return the decoded value.
///
/// Regex: `""|"(?:(?:"")|[^"\\]|\\.)*"`. Value strips the outer quotes then
/// replaces `\"` and `""` with `"`.
fn match_dquote(chars: &[char], start: usize) -> Option<(usize, String)> {
    match_quoted(chars, start, '"')
}

/// Match a single-quoted string and return the decoded value.
fn match_squote(chars: &[char], start: usize) -> Option<(usize, String)> {
    match_quoted(chars, start, '\'')
}

/// Shared quoted-string matcher for `"` and `'`.
fn match_quoted(chars: &[char], start: usize, q: char) -> Option<(usize, String)> {
    if chars[start] != q {
        return None;
    }
    // Empty string token: two quotes.
    if start + 1 < chars.len() && chars[start + 1] == q {
        // Could be an escaped "" inside a longer string, but the regex alternative
        // `""` as a standalone empty string only wins when it is a complete token.
        // moo tries the full alternation; the empty-string alternative matches
        // length 2. The longer alternative may match more, so keep scanning.
        if let Some(res) = scan_quoted_body(chars, start, q) {
            return Some(res);
        }
        // Fall back to empty string.
        return Some((2, String::new()));
    }
    scan_quoted_body(chars, start, q)
}

/// Scan a full quoted body `q ... q` with `""`, escaped `\.`, and other chars.
fn scan_quoted_body(chars: &[char], start: usize, q: char) -> Option<(usize, String)> {
    let mut pos = start + 1;
    let mut value = String::new();
    while pos < chars.len() {
        let c = chars[pos];
        if c == q {
            // A doubled quote inside the string.
            if pos + 1 < chars.len() && chars[pos + 1] == q {
                value.push(q);
                pos += 2;
                continue;
            }
            // Closing quote.
            return Some((pos + 1 - start, value));
        }
        if c == '\\' {
            if pos + 1 < chars.len() {
                let next = chars[pos + 1];
                // `\.` matches any char. The value transform replaces `\"`/`\'`
                // with the bare quote; other escapes keep the backslash.
                if next == q {
                    value.push(q);
                } else {
                    value.push('\\');
                    value.push(next);
                }
                pos += 2;
                continue;
            }
            return None;
        }
        value.push(c);
        pos += 1;
    }
    None
}

/// Match a numeric literal: `[+-]?(?:\d+(?:\.\d+)?(?:[Ee][+-]?\d+)?)`.
fn match_number(chars: &[char], start: usize) -> Option<usize> {
    let mut pos = start;
    if pos < chars.len() && (chars[pos] == '+' || chars[pos] == '-') {
        pos += 1;
    }
    let int_start = pos;
    while pos < chars.len() && chars[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos == int_start {
        return None;
    }
    // Optional fraction.
    if pos < chars.len() && chars[pos] == '.' {
        let mut p = pos + 1;
        let frac_start = p;
        while p < chars.len() && chars[p].is_ascii_digit() {
            p += 1;
        }
        if p > frac_start {
            pos = p;
        }
    }
    // Optional exponent.
    if pos < chars.len() && (chars[pos] == 'e' || chars[pos] == 'E') {
        let mut p = pos + 1;
        if p < chars.len() && (chars[p] == '+' || chars[p] == '-') {
            p += 1;
        }
        let exp_start = p;
        while p < chars.len() && chars[p].is_ascii_digit() {
            p += 1;
        }
        if p > exp_start {
            pos = p;
        }
    }
    Some(pos - start)
}

/// Match a backtick-quoted identifier: `` `(?:(?:``)|[^`\\])*` ``.
///
/// Value strips the outer backticks then replaces `` `` `` with `` ` ``.
/// Backslash does not escape backticks.
fn match_ident_quoted(chars: &[char], start: usize) -> Option<(usize, String)> {
    if chars[start] != '`' {
        return None;
    }
    let mut pos = start + 1;
    let mut value = String::new();
    while pos < chars.len() {
        let c = chars[pos];
        if c == '`' {
            if pos + 1 < chars.len() && chars[pos + 1] == '`' {
                value.push('`');
                pos += 2;
                continue;
            }
            return Some((pos + 1 - start, value));
        }
        if c == '\\' {
            // Backslash is not an escape here. The body class is `[^`\\]`, so a
            // lone backslash cannot appear. moo would not match past it.
            return None;
        }
        value.push(c);
        pos += 1;
    }
    None
}

/// Match an unquoted identifier: `[0-9a-zA-Z$_]+`.
fn match_ident_unquoted(chars: &[char], start: usize) -> Option<usize> {
    let mut pos = start;
    while pos < chars.len() {
        let c = chars[pos];
        if c.is_ascii_alphanumeric() || c == '$' || c == '_' {
            pos += 1;
        } else {
            break;
        }
    }
    if pos > start {
        Some(pos - start)
    } else {
        None
    }
}
