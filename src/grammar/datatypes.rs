//! Datatype grammar rules. Produces `O_DATATYPE` nodes.

use serde_json::{json, Map, Value};

use crate::lexer::TokenKind;

use super::helpers::s_number;
use super::stream::Stream;

/// Parse an optional parenthesized single number: `( n )`. Returns the number.
fn opt_paren_number(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    s.ws0();
    if s.eat(&TokenKind::LParens).is_none() {
        s.set(save);
        return None;
    }
    s.ws0();
    let n = match s_number(s) {
        Some(n) => n,
        None => {
            s.set(save);
            return None;
        }
    };
    s.ws0();
    if s.eat(&TokenKind::RParens).is_none() {
        s.set(save);
        return None;
    }
    Some(n)
}

/// Parse an optional `( d , n )` pair. Returns `(digits, decimals)`.
fn opt_paren_pair(s: &mut Stream) -> Option<(Value, Value)> {
    let save = s.pos();
    s.ws0();
    if s.eat(&TokenKind::LParens).is_none() {
        s.set(save);
        return None;
    }
    s.ws0();
    let d = match s_number(s) {
        Some(v) => v,
        None => {
            s.set(save);
            return None;
        }
    };
    s.ws0();
    if s.eat(&TokenKind::Comma).is_none() {
        s.set(save);
        return None;
    }
    s.ws0();
    let n = match s_number(s) {
        Some(v) => v,
        None => {
            s.set(save);
            return None;
        }
    };
    s.ws0();
    if s.eat(&TokenKind::RParens).is_none() {
        s.set(save);
        return None;
    }
    Some((d, n))
}

/// Consume one keyword from a list, returning its raw value.
fn one_of_keywords(s: &mut Stream, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(v) = s.eat_keyword(name) {
            return Some(v);
        }
    }
    None
}

/// Wrap a datatype subnode in `{id:'O_DATATYPE', def:<sub>}`.
fn wrap(sub: Value) -> Value {
    json!({ "id": "O_DATATYPE", "def": sub })
}

/// Parse `O_DATATYPE`. Tries each subrule in order.
pub fn o_datatype(s: &mut Stream) -> Option<Value> {
    let subrules: &[fn(&mut Stream) -> Option<Value>] = &[
        integer,
        fixed_point,
        floating_point,
        bit,
        boolean,
        datetime,
        year,
        variable_string,
        fixed_string,
        enum_type,
        set_type,
        uuid,
        spatial,
        json_type,
    ];
    for rule in subrules {
        let save = s.pos();
        if let Some(v) = rule(s) {
            return Some(wrap(v));
        }
        s.set(save);
    }
    None
}

fn integer(s: &mut Stream) -> Option<Value> {
    let datatype = one_of_keywords(
        s,
        &[
            "INT",
            "INTEGER",
            "TINYINT",
            "SMALLINT",
            "MEDIUMINT",
            "BIGINT",
        ],
    )?;
    let display_width = opt_paren_number(s);
    let mut def = Map::new();
    def.insert("datatype".into(), Value::String(datatype));
    // The source stores `displayWidth: d[1]` where a failed optional is null,
    // so the field is null when there is no parenthesized width.
    def.insert("displayWidth".into(), display_width.unwrap_or(Value::Null));
    Some(json!({ "id": "O_INTEGER_DATATYPE", "def": def }))
}

fn fixed_point(s: &mut Stream) -> Option<Value> {
    let datatype = one_of_keywords(s, &["DECIMAL", "NUMERIC"])?;
    let mut def = Map::new();
    def.insert("datatype".into(), Value::String(datatype));
    if let Some((d, n)) = opt_paren_pair(s) {
        def.insert("digits".into(), d);
        def.insert("decimals".into(), n);
    } else if let Some(d) = opt_paren_number(s) {
        def.insert("digits".into(), d);
        def.insert("decimals".into(), Value::from(0));
    } else {
        def.insert("digits".into(), Value::from(10));
        def.insert("decimals".into(), Value::from(0));
    }
    Some(json!({ "id": "O_FIXED_POINT_DATATYPE", "def": def }))
}

fn floating_point(s: &mut Stream) -> Option<Value> {
    let datatype = one_of_keywords(s, &["FLOAT", "DOUBLE"])?;
    let mut def = Map::new();
    def.insert("datatype".into(), Value::String(datatype));
    if let Some((d, n)) = opt_paren_pair(s) {
        def.insert("digits".into(), d);
        def.insert("decimals".into(), n);
    }
    Some(json!({ "id": "O_FLOATING_POINT_DATATYPE", "def": def }))
}

fn bit(s: &mut Stream) -> Option<Value> {
    let datatype = s.eat_keyword("BIT")?;
    let length = opt_paren_number(s).unwrap_or(Value::from(1));
    // The bit rule also consumes trailing whitespace after the parens. The
    // grammar has `... %S_RPARENS _`. That is handled by callers via ws.
    Some(json!({
        "id": "O_BIT_DATATYPE",
        "def": { "datatype": datatype, "length": length }
    }))
}

fn boolean(s: &mut Stream) -> Option<Value> {
    let datatype = one_of_keywords(s, &["BOOLEAN", "BOOL"])?;
    Some(json!({
        "id": "O_BOOLEAN_DATATYPE",
        "def": { "datatype": datatype }
    }))
}

fn datetime(s: &mut Stream) -> Option<Value> {
    let datatype = one_of_keywords(s, &["DATE", "TIME", "DATETIME", "TIMESTAMP"])?;
    let fractional = opt_paren_number(s).unwrap_or(Value::from(0));
    Some(json!({
        "id": "O_DATETIME_DATATYPE",
        "def": { "datatype": datatype, "fractional": fractional }
    }))
}

fn year(s: &mut Stream) -> Option<Value> {
    let datatype = s.eat_keyword("YEAR")?;
    let digits = opt_paren_number(s).unwrap_or(Value::from(4));
    Some(json!({
        "id": "O_YEAR_DATATYPE",
        "def": { "datatype": datatype, "digits": digits }
    }))
}

fn variable_string(s: &mut Stream) -> Option<Value> {
    // VARCHAR (n) [BINARY]?
    let save = s.pos();
    if let Some(datatype) = s.eat_keyword("VARCHAR") {
        if let Some(length) = opt_paren_number(s) {
            let mut def = Map::new();
            def.insert("datatype".into(), Value::String(datatype));
            def.insert("length".into(), length);
            // Optional BINARY collation.
            let bsave = s.pos();
            s.ws0();
            if s.eat_keyword("BINARY").is_some() {
                def.insert("binaryCollation".into(), Value::Bool(true));
            } else {
                s.set(bsave);
            }
            return Some(json!({ "id": "O_VARIABLE_STRING_DATATYPE", "def": def }));
        }
        s.set(save);
    }

    // VARBINARY (n)
    let save = s.pos();
    if let Some(datatype) = s.eat_keyword("VARBINARY") {
        if let Some(length) = opt_paren_number(s) {
            return Some(json!({
                "id": "O_VARIABLE_STRING_DATATYPE",
                "def": { "datatype": datatype, "length": length }
            }));
        }
        s.set(save);
    }

    // NCHAR | (NATIONAL CHAR) | NVARCHAR | CHARACTER | CHAR | BINARY, optional (n)
    let datatype = variable_string_name(s)?;
    let length = opt_paren_number(s).unwrap_or(Value::from(1));
    Some(json!({
        "id": "O_VARIABLE_STRING_DATATYPE",
        "def": { "datatype": datatype, "length": length }
    }))
}

/// Parse the leading name of a fixed-length variable string type.
fn variable_string_name(s: &mut Stream) -> Option<String> {
    if let Some(v) = s.eat_keyword("NCHAR") {
        return Some(v);
    }
    // NATIONAL CHAR -> "NATIONAL CHAR".
    let save = s.pos();
    if let Some(nat) = s.eat_keyword("NATIONAL") {
        if s.ws1() {
            if let Some(ch) = s.eat_keyword("CHAR") {
                return Some(format!("{} {}", nat, ch));
            }
        }
        s.set(save);
    }
    if let Some(v) = s.eat_keyword("NVARCHAR") {
        return Some(v);
    }
    if let Some(v) = s.eat_keyword("CHARACTER") {
        return Some(v);
    }
    if let Some(v) = s.eat_keyword("CHAR") {
        return Some(v);
    }
    if let Some(v) = s.eat_keyword("BINARY") {
        return Some(v);
    }
    None
}

fn fixed_string(s: &mut Stream) -> Option<Value> {
    // BLOB | TEXT with optional (n), default 65535.
    if let Some(datatype) = one_of_keywords(s, &["BLOB", "TEXT"]) {
        let length = opt_paren_number(s).unwrap_or(Value::from(65535));
        return Some(json!({
            "id": "O_FIXED_STRING_DATATYPE",
            "def": { "datatype": datatype, "length": length }
        }));
    }
    let fixed: &[(&str, i64)] = &[
        ("TINYBLOB", 255),
        ("MEDIUMBLOB", 16_777_215),
        ("LONGBLOB", 4_294_967_295),
        ("TINYTEXT", 255),
        ("MEDIUMTEXT", 16_777_215),
        ("LONGTEXT", 4_294_967_295),
    ];
    for (name, len) in fixed {
        if let Some(datatype) = s.eat_keyword(name) {
            return Some(json!({
                "id": "O_FIXED_STRING_DATATYPE",
                "def": { "datatype": datatype, "length": len }
            }));
        }
    }
    None
}

/// Parse a comma-separated list of single-quoted strings inside parens.
fn squote_list(s: &mut Stream) -> Option<Vec<String>> {
    let save = s.pos();
    s.ws0();
    if s.eat(&TokenKind::LParens).is_none() {
        s.set(save);
        return None;
    }
    s.ws0();
    let first = match s.eat(&TokenKind::SQuoteString) {
        Some(v) => v,
        None => {
            s.set(save);
            return None;
        }
    };
    let mut values = vec![first];
    loop {
        let item_save = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Comma).is_none() {
            s.set(item_save);
            break;
        }
        s.ws0();
        match s.eat(&TokenKind::SQuoteString) {
            Some(v) => values.push(v),
            None => {
                s.set(item_save);
                break;
            }
        }
    }
    s.ws0();
    if s.eat(&TokenKind::RParens).is_none() {
        s.set(save);
        return None;
    }
    Some(values)
}

fn enum_type(s: &mut Stream) -> Option<Value> {
    let datatype = s.eat_keyword("ENUM")?;
    let values = squote_list(s)?;
    Some(json!({
        "id": "O_ENUM_DATATYPE",
        "def": { "datatype": datatype, "values": values }
    }))
}

fn set_type(s: &mut Stream) -> Option<Value> {
    let datatype = s.eat_keyword("SET")?;
    let values = squote_list(s)?;
    Some(json!({
        "id": "O_SET_DATATYPE",
        "def": { "datatype": datatype, "values": values }
    }))
}

fn uuid(s: &mut Stream) -> Option<Value> {
    let datatype = one_of_keywords(s, &["UUID", "UNIQUEIDENTIFIER"])?;
    Some(json!({
        "id": "O_UUID_DATATYPE",
        "def": { "datatype": datatype }
    }))
}

fn spatial(s: &mut Stream) -> Option<Value> {
    let datatype = one_of_keywords(
        s,
        &[
            "GEOMETRY",
            "POINT",
            "LINESTRING",
            "POLYGON",
            "MULTIPOINT",
            "MULTILINESTRING",
            "MULTIPOLYGON",
            "GEOMETRYCOLLECTION",
        ],
    )?;
    Some(json!({
        "id": "O_SPATIAL_DATATYPE",
        "def": { "datatype": datatype }
    }))
}

fn json_type(s: &mut Stream) -> Option<Value> {
    let datatype = s.eat_keyword("JSON")?;
    Some(json!({
        "id": "O_JSON_DATATYPE",
        "def": { "datatype": datatype }
    }))
}
