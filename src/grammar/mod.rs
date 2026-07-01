//! Recursive-descent grammar that turns a token stream into a parse tree.
//!
//! Each `P_DDS` is one statement. The dispatcher tries statement rules in the
//! source order and takes the first that consumes the whole statement.

mod alter_table;
mod common;
mod create_table;
mod datatypes;
mod helpers;
mod statements;
mod stream;

use serde_json::{json, Value};

use crate::lexer::{tokenize, Token};

use stream::Stream;

/// A parse error carrying the line number where parsing failed.
#[derive(Debug, Clone)]
pub struct ParseError {
    /// Line within the statement, counting from 1.
    pub line: usize,
}

/// Parse one statement string into a `P_DDS` node.
///
/// Leading and trailing whitespace is allowed. On failure the error carries the
/// line number of the offending token, matching the source line reporting.
pub fn parse_statement(input: &str) -> Result<Value, ParseError> {
    let tokens = tokenize(input).map_err(|e| ParseError { line: e.line })?;
    let mut s = Stream::new(&tokens);

    // `P_DDS -> _ ( <statement> _ )`.
    s.ws0();
    let inner = match dispatch(&mut s) {
        Some(v) => v,
        None => {
            return Err(ParseError {
                line: line_at(&tokens, s.pos()),
            })
        }
    };
    s.ws0();

    if !s.at_end() {
        return Err(ParseError {
            line: line_at(&tokens, s.pos()),
        });
    }

    Ok(json!({ "id": "P_DDS", "def": inner }))
}

/// Try each statement rule in source order.
fn dispatch(s: &mut Stream) -> Option<Value> {
    let rules: &[fn(&mut Stream) -> Option<Value>] = &[
        statements::p_create_db,
        create_table::p_create_table,
        statements::p_create_index,
        statements::p_alter_db,
        alter_table::p_alter_table,
        statements::p_drop_db,
        statements::p_drop_table,
        statements::p_drop_index,
        statements::p_rename_table,
        statements::p_set,
        statements::p_use_db,
    ];
    for rule in rules {
        let save = s.pos();
        if let Some(v) = rule(s) {
            return Some(v);
        }
        s.set(save);
    }
    None
}

/// Compute the 1-based line number at a token position by counting newlines in
/// the values consumed up to that point.
fn line_at(tokens: &[Token], pos: usize) -> usize {
    let mut line = 1;
    for t in tokens.iter().take(pos) {
        line += t.value.matches('\n').count();
    }
    line
}
