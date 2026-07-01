//! The public `Parser`: streaming statement splitter plus statement parsing.

use serde_json::{json, Value};

use crate::compact;
use crate::grammar::parse_statement;
use crate::json_schema;

/// Error returned by parser operations.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Copy)]
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
/// let tables = parser.to_compact_json(None).unwrap();
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
    /// semicolons and keeps cross-chunk state. Chainable.
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

    /// Drain fed statements and parse each one into a `P_DDS` node.
    ///
    /// Returns the parse tree root `{ id: "MAIN", def: [...] }`. Consumes the
    /// pending statements. On a parse error the line number is corrected to
    /// stream coordinates and everything resets.
    pub fn results(&mut self) -> Result<Value, Error> {
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
                    // Reset all state, matching the source error path.
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

    /// Format parsed SQL into the compact table model.
    ///
    /// With `None`, parses the fed SQL first. With `Some(tree)`, formats the
    /// given `MAIN` tree.
    pub fn to_compact_json(&mut self, json: Option<Value>) -> Result<Vec<Value>, Error> {
        let tree = match json {
            Some(v) => v,
            None => self.results()?,
        };
        compact::format(&tree).map_err(Error::Format)
    }

    /// Format parsed SQL into JSON Schema draft-07 documents, one per table.
    ///
    /// With `tables = None`, parses the fed SQL and builds the compact model
    /// first. Options default to `{ use_ref: true }`.
    pub fn to_json_schema_array(
        &mut self,
        options: Option<JsonSchemaOptions>,
        tables: Option<Vec<Value>>,
    ) -> Result<Vec<Value>, Error> {
        let opts = options.unwrap_or_default();
        let tables = match tables {
            Some(t) => t,
            None => self.to_compact_json(None)?,
        };
        Ok(json_schema::format(&tables, opts))
    }

    /// Whether the preparser is currently inside an escape. Exposed for tests.
    #[doc(hidden)]
    pub fn is_escaped(&self) -> bool {
        self.escaped
    }

    /// The current open quote char, or none. Exposed for tests.
    #[doc(hidden)]
    pub fn quoted_char(&self) -> Option<char> {
        self.quoted
    }

    /// The number of split statements pending. Exposed for tests.
    #[doc(hidden)]
    pub fn statement_count(&self) -> usize {
        self.statements.len()
    }
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
