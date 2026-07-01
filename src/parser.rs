//! The public `Parser`: streaming statement splitter plus statement parsing.

use serde_json::{json, Value};

use crate::compact;
use crate::grammar::parse_statement;
use crate::json_schema;

/// Error returned by parser operations.
///
/// Marked `#[non_exhaustive]` so new variants can be added without a breaking
/// change. Match arms on this type must include a wildcard.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// An unsupported SQL dialect was given to the constructor.
    UnsupportedDialect(String),
    /// A statement failed to parse. The message contains the line number in
    /// stream coordinates.
    Parse(String),
    /// A formatter received JSON that was not a valid parse tree root.
    Format(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnsupportedDialect(d) => write!(
                f,
                "Unsupported SQL dialect given to parser: '{}. Please provide 'mysql', 'mariadb' or none to use default.",
                d
            ),
            Error::Parse(m) => write!(f, "{}", m),
            Error::Format(m) => write!(f, "{}", m),
        }
    }
}

impl std::error::Error for Error {}

/// Options for JSON Schema output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonSchemaOptions {
    /// Whether to keep column schemas under `definitions` and reference them
    /// with `$ref`. When false, schemas are flattened into `properties`.
    pub use_ref: bool,
}

impl Default for JsonSchemaOptions {
    fn default() -> Self {
        JsonSchemaOptions { use_ref: true }
    }
}

/// Parser for MySQL and MariaDB DDL.
///
/// Feed SQL in one or more chunks, then read the parse tree, the compact table
/// model, or JSON Schema documents.
///
/// ```
/// use sql_ddl_to_json_schema::Parser;
///
/// let mut parser = Parser::new("mysql").unwrap();
/// parser.feed("CREATE TABLE t (id INT PRIMARY KEY);");
/// let tables = parser.parse_compact().unwrap();
/// assert_eq!(tables[0]["name"], "t");
/// ```
pub struct Parser {
    statements: Vec<String>,
    remains: String,
    escaped: bool,
    quoted: Option<char>,
}

impl Parser {
    /// Build a parser for the given dialect.
    ///
    /// An empty string, `"mysql"`, or `"mariadb"` selects the MySQL grammar.
    /// Any other value returns [`Error::UnsupportedDialect`].
    pub fn new(dialect: &str) -> Result<Self, Error> {
        if dialect.is_empty() || dialect == "mysql" || dialect == "mariadb" {
            Ok(Parser {
                statements: Vec::new(),
                remains: String::new(),
                escaped: false,
                quoted: None,
            })
        } else {
            Err(Error::UnsupportedDialect(dialect.to_string()))
        }
    }

    /// Feed a chunk of SQL. Splits input into statements at unquoted, unescaped
    /// semicolons and keeps cross-chunk state. Returns `&mut Self` for chaining.
    pub fn feed(&mut self, chunk: &str) -> &mut Self {
        // Operate on UTF-16 code units to match the source string indexing.
        let units: Vec<u16> = chunk.encode_utf16().collect();
        let mut parsed: Vec<u16> = Vec::with_capacity(units.len());
        let mut last_statement_index = 0usize;

        for (i, &unit) in units.iter().enumerate() {
            parsed.push(unit);
            let ch = char::from_u32(unit as u32);

            if ch == Some('\\') {
                self.escaped = !self.escaped;
            } else {
                if !self.escaped && is_quote_char(ch) {
                    let c = ch.unwrap();
                    match self.quoted {
                        Some(open) => {
                            if open == c {
                                self.quoted = None;
                            }
                        }
                        None => self.quoted = Some(c),
                    }
                } else if ch == Some(';') && self.quoted.is_none() {
                    let slice = &parsed[last_statement_index..i + 1];
                    let statement = format!("{}{}", self.remains, utf16_to_string(slice));
                    self.statements.push(statement);
                    self.remains.clear();
                    last_statement_index = i + 1;
                }
                self.escaped = false;
            }
        }

        let tail = &parsed[last_statement_index..];
        self.remains.push_str(&utf16_to_string(tail));
        self
    }

    /// Drain the buffered statements and parse each into a `P_DDS` node.
    ///
    /// Returns the parse tree root `{ id: "MAIN", def: [...] }`. This consumes
    /// the pending statements, so a second call with no new input returns an
    /// empty tree. On a parse error the line number is reported in stream
    /// coordinates and all state resets.
    ///
    /// ```
    /// use sql_ddl_to_json_schema::Parser;
    ///
    /// let mut parser = Parser::new("mysql").unwrap();
    /// parser.feed("CREATE TABLE t (id INT);");
    /// let first = parser.parse().unwrap();
    /// assert_eq!(first["def"].as_array().unwrap().len(), 1);
    /// // The statements are drained. A second call sees nothing.
    /// let second = parser.parse().unwrap();
    /// assert!(second["def"].as_array().unwrap().is_empty());
    /// ```
    pub fn parse(&mut self) -> Result<Value, Error> {
        let mut line_count = 1usize;
        let mut results = Vec::new();

        let statements = std::mem::take(&mut self.statements);
        for statement in statements {
            if statement.is_empty() {
                break;
            }
            match parse_statement(&statement) {
                Ok(node) => {
                    line_count += count_line_breaks(&statement);
                    results.push(node);
                }
                Err(e) => {
                    let error_line = e.line;
                    let new_count = line_count + error_line - 1;
                    // Reset all state so a failed parse leaves a clean parser.
                    self.statements.clear();
                    self.remains.clear();
                    self.escaped = false;
                    self.quoted = None;
                    return Err(Error::Parse(format!(
                        "invalid syntax at line {}",
                        new_count
                    )));
                }
            }
        }

        self.remains.clear();
        self.escaped = false;
        self.quoted = None;

        Ok(json!({ "id": "MAIN", "def": results }))
    }

    /// Drain the buffered statements and build the compact table model.
    ///
    /// Consumes the pending statements. To format a tree you already hold, use
    /// [`compact_from_tree`].
    pub fn parse_compact(&mut self) -> Result<Vec<Value>, Error> {
        let tree = self.parse()?;
        compact_from_tree(&tree)
    }

    /// Drain the buffered statements and build JSON Schema draft-07 documents,
    /// one per table.
    ///
    /// Consumes the pending statements. Pass [`JsonSchemaOptions::default`] for
    /// the default `{ use_ref: true }`. To format tables you already hold, use
    /// [`json_schema_from_tables`].
    pub fn parse_json_schema(&mut self, options: JsonSchemaOptions) -> Result<Vec<Value>, Error> {
        let tables = self.parse_compact()?;
        Ok(json_schema_from_tables(&tables, options))
    }
}

/// Build the compact table model from a `MAIN` parse tree.
///
/// Use this to format a tree you already hold, for example one returned by
/// [`Parser::parse`]. Returns [`Error::Format`] if the root id is not `MAIN`.
pub fn compact_from_tree(tree: &Value) -> Result<Vec<Value>, Error> {
    compact::format(tree).map_err(Error::Format)
}

/// Build JSON Schema draft-07 documents from compact tables, one per table.
///
/// Use this to format tables you already hold, for example those returned by
/// [`Parser::parse_compact`].
#[must_use]
pub fn json_schema_from_tables(tables: &[Value], options: JsonSchemaOptions) -> Vec<Value> {
    json_schema::format(tables, options)
}

/// Whether a char opens or closes a quoted region.
fn is_quote_char(ch: Option<char>) -> bool {
    matches!(ch, Some('"') | Some('\'') | Some('`'))
}

/// Count `\r\n`, `\r`, and `\n` line breaks the way the source regex does.
fn count_line_breaks(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    i += 2;
                } else {
                    i += 1;
                }
                count += 1;
            }
            b'\n' => {
                count += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    count
}

/// Decode a UTF-16 slice into a string. Invalid units become the replacement
/// character, which does not occur for real DDL input.
fn utf16_to_string(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The preparser tracks escape state, the open quote, and the count of split
    // statements. These fields are private, so the checks live here.

    #[test]
    fn tracks_escape_quote_and_statements() {
        let mut p = Parser::new("mysql").unwrap();

        // Single char.
        p.feed("a");
        assert!(!p.escaped);
        assert_eq!(p.quoted, None);

        // Start escaping.
        p.feed("\\");
        assert!(p.escaped);
        assert_eq!(p.quoted, None);

        // Finish escaping.
        p.feed("\\");
        assert!(!p.escaped);
        assert_eq!(p.quoted, None);

        p.feed("\\");
        assert!(p.escaped);
        p.feed("n");
        assert!(!p.escaped);
        assert_eq!(p.quoted, None);

        // Double quotes without escape.
        p.feed("\"");
        assert!(!p.escaped);
        assert_eq!(p.quoted, Some('"'));
        p.feed("\"");
        assert_eq!(p.quoted, None);
        p.feed("\"a");
        assert_eq!(p.quoted, Some('"'));
        p.feed("a\"");
        assert_eq!(p.quoted, None);

        // Single quotes without escape.
        p.feed("'");
        assert_eq!(p.quoted, Some('\''));
        p.feed("'");
        assert_eq!(p.quoted, None);
        p.feed("'a");
        assert_eq!(p.quoted, Some('\''));
        p.feed("a'");
        assert_eq!(p.quoted, None);

        // Backticks without escape.
        p.feed("`");
        assert_eq!(p.quoted, Some('`'));
        p.feed("`");
        assert_eq!(p.quoted, None);
        p.feed("`a");
        assert_eq!(p.quoted, Some('`'));
        p.feed("a`");
        assert_eq!(p.quoted, None);

        // Quoting with escape.
        p.feed("`\\`");
        assert!(!p.escaped);
        assert_eq!(p.quoted, Some('`'));
        p.feed("\\\\`");
        assert_eq!(p.quoted, None);

        p.feed("\"\\");
        assert!(p.escaped);
        assert_eq!(p.quoted, Some('"'));
        p.feed("\"a`");
        assert_eq!(p.quoted, Some('"'));
        p.feed("'\\'");
        assert_eq!(p.quoted, Some('"'));
        p.feed("\"");
        assert_eq!(p.quoted, None);

        // Escaped semicolon does not split. The next char ends the escape.
        p.feed("\\;");
        assert!(!p.escaped);
        assert_eq!(p.quoted, None);
        assert_eq!(p.statements.len(), 1);

        // Unescaped semicolon splits.
        p.feed("a;");
        assert_eq!(p.statements.len(), 2);

        // Semicolon inside quotes does not split.
        p.feed("a\";");
        assert_eq!(p.quoted, Some('"'));
        assert_eq!(p.statements.len(), 2);

        p.feed("a\";");
        assert_eq!(p.quoted, None);
        assert_eq!(p.statements.len(), 3);
    }

    #[test]
    fn splits_on_utf16_code_units() {
        // Statement splitting counts UTF-16 code units. An astral char is two
        // units, and it must not derail the split at the following semicolon.
        let mut p = Parser::new("mysql").unwrap();
        p.feed("USE \u{1F600};USE b;");
        assert_eq!(p.statements.len(), 2);
    }
}
