//! Public API tests beyond the golden corpus: dialect handling, draining
//! methods versus free functions, streaming, error lines, and datatype edges.

use serde_json::{json, Value};

use sql_ddl_to_json_schema::{
    compact_from_tree, json_schema_from_tables, Error, JsonSchemaOptions, Parser,
};

const SAMPLE: &str =
    "CREATE TABLE t (id INT NOT NULL AUTO_INCREMENT, name VARCHAR(30), PRIMARY KEY (id));";

#[test]
fn unsupported_dialect_errors() {
    match Parser::new("postgres") {
        Err(Error::UnsupportedDialect(d)) => assert_eq!(d, "postgres"),
        other => panic!("expected UnsupportedDialect, got {:?}", other.map(|_| ())),
    }
    // The message matches the documented text.
    let msg = Parser::new("postgres").err().unwrap().to_string();
    assert!(msg.contains("Unsupported SQL dialect given to parser: 'postgres."));
    assert!(msg.contains("Please provide 'mysql', 'mariadb' or none to use default."));
}

#[test]
fn mysql_mariadb_and_empty_dialects_accepted() {
    assert!(Parser::new("mysql").is_ok());
    assert!(Parser::new("mariadb").is_ok());
    assert!(Parser::new("").is_ok());
}

#[test]
fn mysql_equals_mariadb_output() {
    let compact_mysql = {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(SAMPLE);
        p.parse_compact().unwrap()
    };
    let compact_mariadb = {
        let mut p = Parser::new("mariadb").unwrap();
        p.feed(SAMPLE);
        p.parse_compact().unwrap()
    };
    assert_eq!(compact_mysql, compact_mariadb);
}

#[test]
fn default_options_equal_ref_true() {
    let with_default = {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(SAMPLE);
        p.parse_json_schema(JsonSchemaOptions::default()).unwrap()
    };
    let with_ref_true = {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(SAMPLE);
        p.parse_json_schema(JsonSchemaOptions { use_ref: true })
            .unwrap()
    };
    assert_eq!(with_default, with_ref_true);
}

#[test]
fn free_functions_match_draining_methods() {
    // compact_from_tree(&tree) equals parse_compact() for the same input.
    let (draining_compact, from_tree_compact) = {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(SAMPLE);
        let tree = p.parse().unwrap();
        let from_tree = compact_from_tree(&tree).unwrap();

        let mut q = Parser::new("mysql").unwrap();
        q.feed(SAMPLE);
        let draining = q.parse_compact().unwrap();
        (draining, from_tree)
    };
    assert_eq!(draining_compact, from_tree_compact);

    // json_schema_from_tables(&tables, opts) equals parse_json_schema(opts).
    let mut p = Parser::new("mysql").unwrap();
    p.feed(SAMPLE);
    let tables = p.parse_compact().unwrap();
    let from_tables = json_schema_from_tables(&tables, JsonSchemaOptions { use_ref: true });

    let mut q = Parser::new("mysql").unwrap();
    q.feed(SAMPLE);
    let draining = q
        .parse_json_schema(JsonSchemaOptions { use_ref: true })
        .unwrap();
    assert_eq!(from_tables, draining);
}

#[test]
fn empty_and_whitespace_feed_produce_empty_output() {
    for input in ["", "   ", "\n\t  \n"] {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(input);
        let results = p.parse().unwrap();
        assert_eq!(results, json!({ "id": "MAIN", "def": [] }));

        let mut p = Parser::new("mysql").unwrap();
        p.feed(input);
        assert_eq!(p.parse_compact().unwrap(), Vec::<Value>::new());

        let mut p = Parser::new("mysql").unwrap();
        p.feed(input);
        assert_eq!(
            p.parse_json_schema(JsonSchemaOptions::default()).unwrap(),
            Vec::<Value>::new()
        );
    }
}

#[test]
fn multi_chunk_feed_equals_whole() {
    let whole = {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(SAMPLE);
        p.parse().unwrap()
    };
    // Feed in arbitrary chunks.
    let chunked = {
        let mut p = Parser::new("mysql").unwrap();
        let bytes = SAMPLE.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let end = (i + 7).min(bytes.len());
            p.feed(std::str::from_utf8(&bytes[i..end]).unwrap());
            i = end;
        }
        p.parse().unwrap()
    };
    assert_eq!(whole, chunked);
}

#[test]
fn parse_error_reports_line_number() {
    // The failing token `TEST` sits on line 3. The message names that line, not
    // the statement's first line.
    let mut p = Parser::new("mysql").unwrap();
    p.feed("CREATE\nTABLE\nTEST;");
    let err = p.parse().unwrap_err().to_string();
    assert_eq!(err, "invalid syntax at line 3");

    // A single statement whose error is on a later line reports that line.
    let mut p = Parser::new("mysql").unwrap();
    p.feed("CREATE TABLE t (\n a INT,\n b INT,\n @@@ );");
    let err = p.parse().unwrap_err().to_string();
    assert_eq!(err, "invalid syntax at line 4");
}

#[test]
fn integer_ranges_match_documented_bounds() {
    // For each integer type and sign, check the schema min and max.
    let cases: &[(&str, i64, i64, i64, i64)] = &[
        ("TINYINT", -128, 127, 0, 255),
        ("SMALLINT", -32768, 32767, 0, 65535),
        ("MEDIUMINT", -8_388_608, 8_388_607, 0, 16_777_215),
        ("INT", -2_147_483_648, 2_147_483_647, 0, 4_294_967_295),
        (
            "BIGINT",
            -9_007_199_254_740_991,
            9_007_199_254_740_991,
            0,
            9_007_199_254_740_991,
        ),
    ];
    for (ty, smin, smax, umin, umax) in cases {
        let signed = schema_for_column(&format!("CREATE TABLE t (c {});", ty));
        assert_eq!(signed["minimum"], json!(smin), "{} signed min", ty);
        assert_eq!(signed["maximum"], json!(smax), "{} signed max", ty);

        let unsigned = schema_for_column(&format!("CREATE TABLE t (c {} UNSIGNED);", ty));
        assert_eq!(unsigned["minimum"], json!(umin), "{} unsigned min", ty);
        assert_eq!(unsigned["maximum"], json!(umax), "{} unsigned max", ty);
    }
}

#[test]
fn pattern_and_enum_shapes_are_exact() {
    // set -> pattern.
    let set_schema = schema_for_column("CREATE TABLE t (c SET('a','b'));");
    assert_eq!(set_schema["pattern"], json!("^(a|b)(,(a|b))*$"));

    // enum -> enum.
    let enum_schema = schema_for_column("CREATE TABLE t (c ENUM('x','y'));");
    assert_eq!(enum_schema["enum"], json!(["x", "y"]));

    // uuid -> pattern.
    let uuid_schema = schema_for_column("CREATE TABLE t (c UUID);");
    assert_eq!(
        uuid_schema["pattern"],
        json!("^[a-f\\d]{8}-([a-f\\d]{4}-){3}[a-f\\d]{12}$")
    );

    // year -> pattern with digit count.
    let year_schema = schema_for_column("CREATE TABLE t (c YEAR);");
    assert_eq!(year_schema["pattern"], json!("\\d{1,4}"));
    let year2_schema = schema_for_column("CREATE TABLE t (c YEAR(2));");
    assert_eq!(year2_schema["pattern"], json!("\\d{1,2}"));
}

#[test]
fn decimal_and_float_maximum_use_string_method() {
    let decimal = schema_for_column("CREATE TABLE t (c DECIMAL(4,2));");
    assert_eq!(decimal["maximum"], json!(99.99));
    assert_eq!(decimal["minimum"], json!(-99.99));

    let float = schema_for_column("CREATE TABLE t (c FLOAT(6,2));");
    assert_eq!(float["maximum"], json!(9999.99));
}

#[test]
fn compact_formatter_rejects_non_main_root() {
    // A tree whose root id is not "MAIN" must be rejected with the documented
    // message. This is the only error path in the compact formatter.
    let bad = json!({ "id": "P_DDS", "def": [] });
    let err = compact_from_tree(&bad).unwrap_err();
    assert!(matches!(err, Error::Format(_)), "got {:?}", err);
    assert_eq!(
        err.to_string(),
        "Invalid JSON format provided for CompactFormatter. \
         Please provide JSON from root element, containing { id: MAIN }."
    );
}

#[test]
fn astral_char_round_trips_through_compact() {
    // An astral char (2 UTF-16 units) inside a string default must survive the
    // statement split and land unchanged in the compact model. The split-count
    // check lives in the parser unit tests where the fields are reachable.
    let mut q = Parser::new("mysql").unwrap();
    q.feed("CREATE TABLE t (c VARCHAR(10) DEFAULT '\u{1F600}');");
    let tables = q.parse_compact().unwrap();
    assert_eq!(
        tables[0]["columns"][0]["options"]["default"],
        json!("\u{1F600}")
    );
}

// Parse errors carry a line number in stream coordinates. The number is the
// line of the token where parsing broke, not the line of the statement's first
// token. The grammar tracks the furthest-reached token for this.
#[test]
fn parse_error_line_matches_stream_coordinates() {
    let cases: &[(&[&str], i64)] = &[
        (
            &["CREATE\n        TABLE A (\n        A bool,\n        B bool\n        )\n        ;\n\n      CREATE\n      TEST;\n\n      "],
            9,
        ),
        (
            &["\n        CREATE\n        TEST;\n\n        CREATE TABLE A (\n        A bool,\n        B bool\n        )\n        ;\n      "],
            3,
        ),
        (&["CREATE TABLE A (A bool);\n\r\r\n", "CREATE\n      TEST;"], 4),
    ];
    for (chunks, want) in cases {
        let mut p = Parser::new("mysql").unwrap();
        for c in *chunks {
            p.feed(c);
        }
        let msg = p.parse().unwrap_err().to_string();
        let got: i64 = msg
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(-1);
        assert_eq!(got, *want, "line number for {:?}", chunks);
    }
}

/// Build the JSON Schema for the single column `c` in a one-column table.
fn schema_for_column(ddl: &str) -> Value {
    let mut p = Parser::new("mysql").unwrap();
    p.feed(ddl);
    let schemas = p.parse_json_schema(JsonSchemaOptions::default()).unwrap();
    schemas[0]["definitions"]["c"].clone()
}
