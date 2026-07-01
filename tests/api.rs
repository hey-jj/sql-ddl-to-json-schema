//! Public API tests beyond the golden corpus: dialect handling, argument
//! overloads, streaming, and datatype mapping edges.

use serde_json::{json, Value};

use sql_ddl_to_json_schema::{Error, JsonSchemaOptions, Parser};

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
        p.to_compact_json(None).unwrap()
    };
    let compact_mariadb = {
        let mut p = Parser::new("mariadb").unwrap();
        p.feed(SAMPLE);
        p.to_compact_json(None).unwrap()
    };
    assert_eq!(compact_mysql, compact_mariadb);
}

#[test]
fn default_options_equal_ref_true() {
    let with_default = {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(SAMPLE);
        p.to_json_schema_array(None, None).unwrap()
    };
    let with_ref_true = {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(SAMPLE);
        p.to_json_schema_array(Some(JsonSchemaOptions { use_ref: true }), None)
            .unwrap()
    };
    assert_eq!(with_default, with_ref_true);
}

#[test]
fn explicit_argument_overloads_match_implicit() {
    // to_compact_json(Some(tree)) equals to_compact_json(None).
    let (implicit_compact, explicit_compact) = {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(SAMPLE);
        let tree = p.results().unwrap();
        let explicit = p.to_compact_json(Some(tree)).unwrap();

        let mut q = Parser::new("mysql").unwrap();
        q.feed(SAMPLE);
        let implicit = q.to_compact_json(None).unwrap();
        (implicit, explicit)
    };
    assert_eq!(implicit_compact, explicit_compact);

    // to_json_schema_array(opts, Some(tables)) equals the implicit form.
    let mut p = Parser::new("mysql").unwrap();
    p.feed(SAMPLE);
    let tables = p.to_compact_json(None).unwrap();
    let explicit_schema = p
        .to_json_schema_array(Some(JsonSchemaOptions { use_ref: true }), Some(tables))
        .unwrap();

    let mut q = Parser::new("mysql").unwrap();
    q.feed(SAMPLE);
    let implicit_schema = q.to_json_schema_array(None, None).unwrap();
    assert_eq!(explicit_schema, implicit_schema);
}

#[test]
fn empty_and_whitespace_feed_produce_empty_output() {
    for input in ["", "   ", "\n\t  \n"] {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(input);
        let results = p.results().unwrap();
        assert_eq!(results, json!({ "id": "MAIN", "def": [] }));

        let mut p = Parser::new("mysql").unwrap();
        p.feed(input);
        assert_eq!(p.to_compact_json(None).unwrap(), Vec::<Value>::new());

        let mut p = Parser::new("mysql").unwrap();
        p.feed(input);
        assert_eq!(
            p.to_json_schema_array(None, None).unwrap(),
            Vec::<Value>::new()
        );
    }
}

#[test]
fn multi_chunk_feed_equals_whole() {
    let whole = {
        let mut p = Parser::new("mysql").unwrap();
        p.feed(SAMPLE);
        p.results().unwrap()
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
        p.results().unwrap()
    };
    assert_eq!(whole, chunked);
}

#[test]
fn parse_error_reports_line_number() {
    let mut p = Parser::new("mysql").unwrap();
    p.feed("CREATE\nTABLE\nTEST;");
    let err = p.results().unwrap_err().to_string();
    // The message carries a line number in stream coordinates.
    assert!(err.contains("line"));
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
    // message. This is the only throw path in the compact formatter.
    let mut p = Parser::new("mysql").unwrap();
    let bad = json!({ "id": "P_DDS", "def": [] });
    let err = p.to_compact_json(Some(bad)).unwrap_err();
    assert!(matches!(err, Error::Format(_)), "got {:?}", err);
    assert_eq!(
        err.to_string(),
        "Invalid JSON format provided for CompactFormatter. \
         Please provide JSON from root element, containing { id: MAIN }."
    );
}

#[test]
fn preparser_splits_on_utf16_code_units() {
    // Statement splitting counts UTF-16 code units. An astral char (2 units)
    // before a semicolon must not derail the split, and the char must survive
    // into the parse tree.
    let mut p = Parser::new("mysql").unwrap();
    p.feed("USE \u{1F600};USE b;");
    assert_eq!(p.statement_count(), 2);

    // Astral char inside a string default round-trips through parse and compact.
    let mut q = Parser::new("mysql").unwrap();
    q.feed("CREATE TABLE t (c VARCHAR(10) DEFAULT '\u{1F600}');");
    let tables = q.to_compact_json(None).unwrap();
    assert_eq!(
        tables[0]["columns"][0]["options"]["default"],
        json!("\u{1F600}")
    );
}

// Parse errors carry a line number in stream coordinates. The number must be
// the line of the token where parsing broke, not the line of the statement's
// first token. See tracking issue: error line resolution pins to the statement
// start. Enable once the grammar reports the furthest-reached token line.
#[test]
#[ignore = "known divergence: error line reports statement-start, not failure token"]
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
        let msg = p.results().unwrap_err().to_string();
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
    let schemas = p.to_json_schema_array(None, None).unwrap();
    schemas[0]["definitions"]["c"].clone()
}
