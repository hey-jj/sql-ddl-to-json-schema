# sql-ddl-to-json-schema

Parse MySQL and MariaDB DDL into a compact table model and JSON Schema draft-07
documents.

Feed one or more SQL Data Definition statements into a `Parser`, then read one
of three outputs:

- the parse tree, via `results`
- the compact table model, via `to_compact_json`
- JSON Schema draft-07 documents, one per table, via `to_json_schema_array`

Statements replay against an in-memory database, so `ALTER TABLE`, `CREATE
INDEX`, `DROP TABLE`, `RENAME TABLE`, and `CREATE TABLE ... LIKE` all mutate the
accumulated tables.

Only DDL is supported. `SET` parses but is ignored. Database statements (`USE`,
`CREATE DATABASE`, `ALTER DATABASE`, `DROP DATABASE`) parse but produce no
tables.

## Install

```toml
[dependencies]
sql-ddl-to-json-schema = "0.1"
```

## Usage

```rust
use sql_ddl_to_json_schema::Parser;

let ddl = "CREATE TABLE users (id INT NOT NULL AUTO_INCREMENT, \
           nickname VARCHAR(255) NOT NULL, PRIMARY KEY (id)) \
           ENGINE MyISAM COMMENT 'All system users'; \
           ALTER TABLE users ADD UNIQUE KEY unq_nick (nickname);";

let mut parser = Parser::new("mysql").unwrap();
parser.feed(ddl);

let tables = parser.to_compact_json(None).unwrap();
assert_eq!(tables[0]["name"], "users");
```

JSON Schema output:

```rust
use sql_ddl_to_json_schema::{JsonSchemaOptions, Parser};

let mut parser = Parser::new("mysql").unwrap();
parser.feed("CREATE TABLE t (id INT PRIMARY KEY, name VARCHAR(30));");

// Default keeps column schemas under `definitions` with `$ref`.
let with_ref = parser.to_json_schema_array(None, None).unwrap();

// Flatten column schemas into `properties`.
let mut parser = Parser::new("mysql").unwrap();
parser.feed("CREATE TABLE t (id INT PRIMARY KEY, name VARCHAR(30));");
let flattened = parser
    .to_json_schema_array(Some(JsonSchemaOptions { use_ref: false }), None)
    .unwrap();
```

## Streaming

Input can arrive in chunks. Statements split at unquoted, unescaped semicolons.
Cross-chunk state tracks the current quote and escape.

```rust
use sql_ddl_to_json_schema::Parser;

let mut parser = Parser::new("mysql").unwrap();
parser.feed("CREATE TABLE ");
parser.feed("t (id INT);");
let tables = parser.to_compact_json(None).unwrap();
assert_eq!(tables[0]["name"], "t");
```

## Dialects

`Parser::new` accepts `"mysql"`, `"mariadb"`, or an empty string, all of which
use the same grammar. Any other value returns `Error::UnsupportedDialect`.

## Datatype mapping

Integer types map to `integer` with `minimum` and `maximum` from the MySQL
ranges. `bigint` uses the JavaScript safe-integer range
(`+/- 9007199254740991`). `decimal` and `float` compute `maximum` from the digit
and decimal counts. `date`, `time`, and `datetime` map to the draft-07 `format`
values. `enum` maps to `enum`, `set` to a `pattern`, `uuid` to a `pattern`, and
`year` to a digit-count `pattern`.

## Output types

Every output is a `serde_json::Value`. The compact model and JSON Schema
documents build keys in a fixed order that reads the same as the model builds
them.

## License

MIT
