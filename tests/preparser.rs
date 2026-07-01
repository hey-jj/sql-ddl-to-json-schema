//! Unit tests for the streaming pre-parser: escaping, quoting, and splitting.
//!
//! After each `feed`, the escaped flag, the open quote, and the statement count
//! are checked.

use sql_ddl_to_json_schema::Parser;

#[test]
fn tracks_escape_quote_and_statements() {
    let mut p = Parser::new("mysql").unwrap();

    // Single char.
    p.feed("a");
    assert!(!p.is_escaped());
    assert_eq!(p.quoted_char(), None);

    // Start escaping.
    p.feed("\\");
    assert!(p.is_escaped());
    assert_eq!(p.quoted_char(), None);

    // Finish escaping.
    p.feed("\\");
    assert!(!p.is_escaped());
    assert_eq!(p.quoted_char(), None);

    p.feed("\\");
    assert!(p.is_escaped());
    p.feed("n");
    assert!(!p.is_escaped());
    assert_eq!(p.quoted_char(), None);

    // Double quotes without escape.
    p.feed("\"");
    assert!(!p.is_escaped());
    assert_eq!(p.quoted_char(), Some('"'));
    p.feed("\"");
    assert_eq!(p.quoted_char(), None);
    p.feed("\"a");
    assert_eq!(p.quoted_char(), Some('"'));
    p.feed("a\"");
    assert_eq!(p.quoted_char(), None);

    // Single quotes without escape.
    p.feed("'");
    assert_eq!(p.quoted_char(), Some('\''));
    p.feed("'");
    assert_eq!(p.quoted_char(), None);
    p.feed("'a");
    assert_eq!(p.quoted_char(), Some('\''));
    p.feed("a'");
    assert_eq!(p.quoted_char(), None);

    // Backticks without escape.
    p.feed("`");
    assert_eq!(p.quoted_char(), Some('`'));
    p.feed("`");
    assert_eq!(p.quoted_char(), None);
    p.feed("`a");
    assert_eq!(p.quoted_char(), Some('`'));
    p.feed("a`");
    assert_eq!(p.quoted_char(), None);

    // Quoting with escape.
    p.feed("`\\`");
    assert!(!p.is_escaped());
    assert_eq!(p.quoted_char(), Some('`'));
    p.feed("\\\\`");
    assert_eq!(p.quoted_char(), None);

    p.feed("\"\\");
    assert!(p.is_escaped());
    assert_eq!(p.quoted_char(), Some('"'));
    p.feed("\"a`");
    assert_eq!(p.quoted_char(), Some('"'));
    p.feed("'\\'");
    assert_eq!(p.quoted_char(), Some('"'));
    p.feed("\"");
    assert_eq!(p.quoted_char(), None);

    // Semicolon with escape does not split, but the following char does end it.
    p.feed("\\;");
    assert!(!p.is_escaped());
    assert_eq!(p.quoted_char(), None);
    assert_eq!(p.statement_count(), 1);

    // Semicolon without escape splits.
    p.feed("a;");
    assert_eq!(p.statement_count(), 2);

    // Semicolon inside quotes does not split.
    p.feed("a\";");
    assert_eq!(p.quoted_char(), Some('"'));
    assert_eq!(p.statement_count(), 2);

    p.feed("a\";");
    assert_eq!(p.quoted_char(), None);
    assert_eq!(p.statement_count(), 3);
}
