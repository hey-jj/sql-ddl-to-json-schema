//! Parse MySQL and MariaDB DDL into a compact table model and JSON Schema.
//!
//! Feed SQL Data Definition statements into a [`Parser`], then read one of
//! three outputs:
//!
//! - the parse tree, via [`Parser::parse`]
//! - the compact table model, via [`Parser::parse_compact`]
//! - JSON Schema draft-07 documents, via [`Parser::parse_json_schema`]
//!
//! Each of these drains the buffered statements. To reformat a tree you already
//! hold, use the free functions [`compact_from_tree`] and
//! [`json_schema_from_tables`].
//!
//! Only DDL is supported. `SET` parses but is ignored. Database statements
//! (`USE`, `CREATE DATABASE`, `ALTER DATABASE`, `DROP DATABASE`) parse but do
//! not produce tables.
//!
//! The parser is stream friendly. Input can arrive in chunks. Statements split
//! at unquoted, unescaped semicolons.
//!
//! # Example
//!
//! ```
//! use sql_ddl_to_json_schema::Parser;
//!
//! let ddl = "CREATE TABLE users (id INT NOT NULL AUTO_INCREMENT, \
//!            nickname VARCHAR(255) NOT NULL, PRIMARY KEY (id));";
//!
//! let mut parser = Parser::new("mysql").unwrap();
//! parser.feed(ddl);
//! let schemas = parser.parse_json_schema(Default::default()).unwrap();
//! assert_eq!(schemas[0]["$id"], "users");
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod compact;
mod grammar;
mod json_schema;
mod keywords;
mod lexer;
mod parser;

pub use parser::{compact_from_tree, json_schema_from_tables, Error, JsonSchemaOptions, Parser};
