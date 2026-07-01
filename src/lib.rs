//! Parse MySQL and MariaDB DDL into a compact table model and JSON Schema.
//!
//! Feed SQL Data Definition statements into a [`Parser`], then read one of
//! three outputs:
//!
//! - the parse tree, via [`Parser::results`]
//! - the compact table model, via [`Parser::to_compact_json`]
//! - JSON Schema draft-07 documents, via [`Parser::to_json_schema_array`]
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
//! let schemas = parser.to_json_schema_array(None, None).unwrap();
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

pub use parser::{Error, JsonSchemaOptions, Parser};
