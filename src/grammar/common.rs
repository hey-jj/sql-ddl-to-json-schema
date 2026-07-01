//! Grammar rules shared across statements: index columns, index options,
//! column definitions, and column references.

use serde_json::{json, Map, Value};

use crate::lexer::TokenKind;

use super::datatypes::o_datatype;
use super::helpers::{o_quoted_string, o_string_or_ident, s_identifier, s_number};
use super::stream::Stream;

/// Consume one keyword from a list, returning its raw value.
pub fn one_of_keywords(s: &mut Stream, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(v) = s.eat_keyword(name) {
            return Some(v);
        }
    }
    None
}

/// Match the separator `( __ | _ = _ )` used between an option and its value.
///
/// The source alternation prefers whitespace, but a following `=` means the
/// equals branch. Consume whitespace, then take the equals branch if an `=`
/// follows, else require that some whitespace was present.
pub fn ws_or_equals(s: &mut Stream) -> bool {
    let save = s.pos();
    let had_ws = s.ws1();
    if s.eat(&TokenKind::Equal).is_some() {
        s.ws0();
        return true;
    }
    if had_ws {
        return true;
    }
    s.set(save);
    false
}

/// Parse `P_INDEX_COLUMN`: `ident [ ( n ) ]? [ASC|DESC]?`.
pub fn p_index_column(s: &mut Stream) -> Option<Value> {
    let column = s_identifier(s)?;
    let mut length: Option<Value> = None;
    let mut sort: Option<String> = None;

    // Optional `( n )`.
    let save = s.pos();
    s.ws0();
    if s.eat(&TokenKind::LParens).is_some() {
        s.ws0();
        if let Some(n) = s_number(s) {
            s.ws0();
            if s.eat(&TokenKind::RParens).is_some() {
                length = Some(n);
            } else {
                s.set(save);
            }
        } else {
            s.set(save);
        }
    } else {
        s.set(save);
    }

    // Optional ASC or DESC.
    let save = s.pos();
    s.ws0();
    if let Some(v) = one_of_keywords(s, &["ASC", "DESC"]) {
        sort = Some(v);
    } else {
        s.set(save);
    }

    let mut def = Map::new();
    def.insert("column".into(), Value::String(column));
    if let Some(l) = length {
        def.insert("length".into(), l);
    }
    if let Some(so) = sort {
        def.insert("sort".into(), Value::String(so));
    }
    Some(json!({ "id": "P_INDEX_COLUMN", "def": def }))
}

/// Parse a parenthesized, comma-separated list of index columns.
pub fn index_column_list(s: &mut Stream) -> Option<Vec<Value>> {
    let save = s.pos();
    s.ws0();
    if s.eat(&TokenKind::LParens).is_none() {
        s.set(save);
        return None;
    }
    s.ws0();
    let first = match p_index_column(s) {
        Some(v) => v,
        None => {
            s.set(save);
            return None;
        }
    };
    let mut cols = vec![first];
    loop {
        let item = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Comma).is_none() {
            s.set(item);
            break;
        }
        s.ws0();
        match p_index_column(s) {
            Some(v) => cols.push(v),
            None => {
                s.set(item);
                break;
            }
        }
    }
    s.ws0();
    if s.eat(&TokenKind::RParens).is_none() {
        s.set(save);
        return None;
    }
    Some(cols)
}

/// Parse `P_INDEX_TYPE`: `USING (BTREE|HASH|RTREE)`.
pub fn p_index_type(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    s.eat_keyword("USING")?;
    if !s.ws1() {
        s.set(save);
        return None;
    }
    match one_of_keywords(s, &["BTREE", "HASH", "RTREE"]) {
        Some(v) => Some(json!({ "id": "P_INDEX_TYPE", "def": v })),
        None => {
            s.set(save);
            None
        }
    }
}

/// Parse `O_INDEX_OPTION`, one of key block size, index type, parser, comment.
pub fn o_index_option(s: &mut Stream) -> Option<Value> {
    let save = s.pos();

    // KEY_BLOCK_SIZE ( __ | = ) NUMBER
    if s.eat_keyword("KEY_BLOCK_SIZE").is_some() {
        if ws_or_equals(s) {
            if let Some(n) = s_number(s) {
                return Some(json!({ "id": "O_INDEX_OPTION", "def": { "keyBlockSize": n } }));
            }
        }
        s.set(save);
    }

    // P_INDEX_TYPE
    if let Some(it) = p_index_type(s) {
        return Some(json!({ "id": "O_INDEX_OPTION", "def": { "indexType": it } }));
    }
    s.set(save);

    // WITH PARSER ident
    if s.eat_keyword("WITH").is_some() {
        if s.ws1() && s.eat_keyword("PARSER").is_some() && s.ws1() {
            if let Some(p) = s_identifier(s) {
                return Some(json!({ "id": "O_INDEX_OPTION", "def": { "parser": p } }));
            }
        }
        s.set(save);
    }

    // COMMENT string
    if s.eat_keyword("COMMENT").is_some() {
        if s.ws1() {
            if let Some(c) = o_quoted_string(s) {
                return Some(json!({ "id": "O_INDEX_OPTION", "def": { "comment": c } }));
            }
        }
        s.set(save);
    }

    None
}

/// Parse zero or more index options, each preceded by optional whitespace.
pub fn index_options(s: &mut Stream) -> Vec<Value> {
    let mut opts = Vec::new();
    loop {
        let save = s.pos();
        s.ws0();
        match o_index_option(s) {
            Some(o) => opts.push(o),
            None => {
                s.set(save);
                break;
            }
        }
    }
    opts
}

/// Parse `P_INDEX_ALGORITHM_OPTION`: `ALGORITHM ( __ | = ) (DEFAULT|INPLACE|COPY)`.
pub fn p_index_algorithm_option(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    s.eat_keyword("ALGORITHM")?;
    if !ws_or_equals(s) {
        s.set(save);
        return None;
    }
    match one_of_keywords(s, &["DEFAULT", "INPLACE", "COPY"]) {
        Some(v) => Some(json!({
            "id": "P_INDEX_ALGORITHM_OPTION",
            "def": { "algorithm": v }
        })),
        None => {
            s.set(save);
            None
        }
    }
}

/// Parse `P_LOCK_OPTION`: `LOCK ( __ | = ) (DEFAULT|NONE|SHARED|EXCLUSIVE)`.
pub fn p_lock_option(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    s.eat_keyword("LOCK")?;
    if !ws_or_equals(s) {
        s.set(save);
        return None;
    }
    match one_of_keywords(s, &["DEFAULT", "NONE", "SHARED", "EXCLUSIVE"]) {
        Some(v) => Some(json!({ "id": "P_LOCK_OPTION", "def": { "lock": v } })),
        None => {
            s.set(save);
            None
        }
    }
}

/// Parse `O_DEFAULT_VALUE`.
///
/// Order: number, bit literal, hexa literal, identifier with optional `(n)`,
/// then quoted string.
pub fn o_default_value(s: &mut Stream) -> Option<Value> {
    if let Some(n) = s_number(s) {
        return Some(n);
    }
    if let Some(v) = s.eat(&TokenKind::BitFormat) {
        return Some(Value::String(v));
    }
    if let Some(v) = s.eat(&TokenKind::HexaFormat) {
        return Some(Value::String(v));
    }
    let save = s.pos();
    if let Some(ident) = s_identifier(s) {
        // Optional `( num? )`.
        let mut suffix = String::new();
        let psave = s.pos();
        s.ws0();
        if s.eat(&TokenKind::LParens).is_some() {
            s.ws0();
            let num = s.eat(&TokenKind::Number);
            s.ws0();
            if s.eat(&TokenKind::RParens).is_some() {
                suffix = format!("({})", num.unwrap_or_default());
            } else {
                s.set(psave);
            }
        } else {
            s.set(psave);
        }
        return Some(Value::String(format!("{}{}", ident, suffix)));
    }
    s.set(save);
    o_quoted_string(s).map(Value::String)
}

/// Parse `O_COLUMN_DEFINITION`, one column attribute.
pub fn o_column_definition(s: &mut Stream) -> Option<Value> {
    let save = s.pos();

    if s.eat_keyword("UNSIGNED").is_some() {
        return Some(col_def(json!({ "unsigned": true })));
    }
    if s.eat_keyword("ZEROFILL").is_some() {
        return Some(col_def(json!({ "zerofill": true })));
    }

    // CHARSET charset
    if s.eat_keyword("CHARSET").is_some() {
        if s.ws1() {
            if let Some(cs) = o_string_or_ident(s) {
                return Some(col_def(json!({ "charset": cs })));
            }
        }
        s.set(save);
    }

    // CHARACTER SET charset
    if s.eat_keyword("CHARACTER").is_some() {
        if s.ws1() && s.eat_keyword("SET").is_some() && s.ws1() {
            if let Some(cs) = o_string_or_ident(s) {
                return Some(col_def(json!({ "charset": cs })));
            }
        }
        s.set(save);
    }

    // COLLATE collation
    if s.eat_keyword("COLLATE").is_some() {
        if s.ws1() {
            if let Some(c) = o_string_or_ident(s) {
                return Some(col_def(json!({ "collation": c })));
            }
        }
        s.set(save);
    }

    // NOT NULL
    if s.eat_keyword("NOT").is_some() {
        if s.ws1() && s.eat_keyword("NULL").is_some() {
            return Some(col_def(json!({ "nullable": false })));
        }
        s.set(save);
    }

    // NULL
    if s.eat_keyword("NULL").is_some() {
        return Some(col_def(json!({ "nullable": true })));
    }

    // DEFAULT value
    if s.eat_keyword("DEFAULT").is_some() {
        if s.ws1() {
            if let Some(v) = o_default_value(s) {
                return Some(col_def(json!({ "default": v })));
            }
        }
        s.set(save);
    }

    if s.eat_keyword("AUTO_INCREMENT").is_some() {
        return Some(col_def(json!({ "autoincrement": true })));
    }

    // UNIQUE [KEY]?
    if s.eat_keyword("UNIQUE").is_some() {
        let ksave = s.pos();
        s.ws1();
        if s.eat_keyword("KEY").is_none() {
            s.set(ksave);
        }
        return Some(col_def(json!({ "unique": true })));
    }

    // [PRIMARY]? KEY
    let psave = s.pos();
    if s.eat_keyword("PRIMARY").is_some() {
        if s.ws1() && s.eat_keyword("KEY").is_some() {
            return Some(col_def(json!({ "primary": true })));
        }
        s.set(psave);
    }
    if s.eat_keyword("KEY").is_some() {
        return Some(col_def(json!({ "primary": true })));
    }

    // COMMENT string
    if s.eat_keyword("COMMENT").is_some() {
        if s.ws1() {
            if let Some(c) = o_quoted_string(s) {
                return Some(col_def(json!({ "comment": c })));
            }
        }
        s.set(save);
    }

    // INVISIBLE WITH SYSTEM VERSIONING / INVISIBLE WITHOUT SYSTEM VERSIONING / INVISIBLE
    if s.eat_keyword("INVISIBLE").is_some() {
        // Try INVISIBLE WITH SYSTEM VERSIONING.
        let isave = s.pos();
        if s.ws1()
            && s.eat_keyword("WITH").is_some()
            && s.ws1()
            && s.eat_keyword("SYSTEM").is_some()
            && s.ws1()
            && s.eat_keyword("VERSIONING").is_some()
        {
            return Some(col_def(json!({ "invisibleWithSystemVersioning": true })));
        }
        // Try INVISIBLE WITHOUT SYSTEM VERSIONING.
        s.set(isave);
        if s.ws1()
            && s.eat_keyword("WITHOUT").is_some()
            && s.ws1()
            && s.eat_keyword("SYSTEM").is_some()
            && s.ws1()
            && s.eat_keyword("VERSIONING").is_some()
        {
            return Some(col_def(json!({ "invisibleWithoutSystemVersioning": true })));
        }
        // Bare INVISIBLE.
        s.set(isave);
        return Some(col_def(json!({ "invisible": true })));
    }

    // COLUMN_FORMAT (FIXED|DYNAMIC|DEFAULT)
    if s.eat_keyword("COLUMN_FORMAT").is_some() {
        if s.ws1() {
            if let Some(v) = one_of_keywords(s, &["FIXED", "DYNAMIC", "DEFAULT"]) {
                return Some(col_def(json!({ "format": v })));
            }
        }
        s.set(save);
    }

    // STORAGE (DISK|MEMORY|DEFAULT)
    if s.eat_keyword("STORAGE").is_some() {
        if s.ws1() {
            if let Some(v) = one_of_keywords(s, &["DISK", "MEMORY", "DEFAULT"]) {
                return Some(col_def(json!({ "storage": v })));
            }
        }
        s.set(save);
    }

    // ON UPDATE CURRENT_TIMESTAMP [ ( n? ) ]?
    if s.eat_keyword("ON").is_some() {
        if s.ws1()
            && s.eat_keyword("UPDATE").is_some()
            && s.ws1()
            && s.eat_keyword("CURRENT_TIMESTAMP").is_some()
        {
            let mut on_update = "CURRENT_TIMESTAMP".to_string();
            let osave = s.pos();
            s.ws0();
            if s.eat(&TokenKind::LParens).is_some() {
                s.ws0();
                let num = s.eat(&TokenKind::Number);
                s.ws0();
                if s.eat(&TokenKind::RParens).is_some() {
                    on_update.push_str(&format!("({})", num.unwrap_or_default()));
                } else {
                    s.set(osave);
                }
            } else {
                s.set(osave);
            }
            return Some(col_def(json!({ "onUpdate": on_update })));
        }
        s.set(save);
    }

    None
}

/// Wrap a column definition attribute node.
fn col_def(def: Value) -> Value {
    json!({ "id": "O_COLUMN_DEFINITION", "def": def })
}

/// Parse zero or more column definitions, each preceded by required whitespace.
pub fn column_definitions(s: &mut Stream) -> Vec<Value> {
    let mut defs = Vec::new();
    loop {
        let save = s.pos();
        if !s.ws1() {
            s.set(save);
            break;
        }
        match o_column_definition(s) {
            Some(d) => defs.push(d),
            None => {
                s.set(save);
                break;
            }
        }
    }
    defs
}

/// Parse `P_COLUMN_REFERENCE`.
pub fn p_column_reference(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    s.eat_keyword("REFERENCES")?;
    if !s.ws1() {
        s.set(save);
        return None;
    }
    let table = match s_identifier(s) {
        Some(t) => t,
        None => {
            s.set(save);
            return None;
        }
    };

    // Columns: `( col (, col)* )` then trailing whitespace.
    let columns = match reference_columns(s) {
        Some(c) => c,
        None => {
            s.set(save);
            return None;
        }
    };

    // Optional MATCH (FULL|PARTIAL|SIMPLE).
    let mut match_val: Option<String> = None;
    let msave = s.pos();
    if s.eat_keyword("MATCH").is_some() {
        if s.ws1() {
            if let Some(v) = one_of_keywords(s, &["FULL", "PARTIAL", "SIMPLE"]) {
                match_val = Some(v);
                s.ws0();
            } else {
                s.set(msave);
            }
        } else {
            s.set(msave);
        }
    } else {
        s.set(msave);
    }

    // Zero or more ON (DELETE|UPDATE) action.
    let mut on = Vec::new();
    loop {
        let osave = s.pos();
        if s.eat_keyword("ON").is_none() {
            s.set(osave);
            break;
        }
        if !s.ws1() {
            s.set(osave);
            break;
        }
        let trigger = match one_of_keywords(s, &["DELETE", "UPDATE"]) {
            Some(v) => v,
            None => {
                s.set(osave);
                break;
            }
        };
        if !s.ws1() {
            s.set(osave);
            break;
        }
        let action = match reference_action(s) {
            Some(v) => v,
            None => {
                s.set(osave);
                break;
            }
        };
        s.ws0();
        on.push(json!({ "trigger": trigger, "action": action }));
    }

    let mut def = Map::new();
    def.insert("table".into(), Value::String(table));
    def.insert("columns".into(), Value::Array(columns));
    // A failed optional yields null in the parse tree, so `match` is null when
    // absent rather than dropped.
    def.insert(
        "match".into(),
        match_val.map(Value::String).unwrap_or(Value::Null),
    );
    def.insert("on".into(), Value::Array(on));
    Some(json!({ "id": "P_COLUMN_REFERENCE", "def": def }))
}

/// Parse the parenthesized reference column list with trailing whitespace.
fn reference_columns(s: &mut Stream) -> Option<Vec<Value>> {
    let cols = index_column_list(s)?;
    // The grammar consumes trailing `_` after the closing paren.
    s.ws0();
    Some(cols)
}

/// Parse a reference action keyword phrase.
fn reference_action(s: &mut Stream) -> Option<String> {
    let save = s.pos();
    if let Some(v) = s.eat_keyword("RESTRICT") {
        return Some(v);
    }
    if let Some(v) = s.eat_keyword("CASCADE") {
        return Some(v);
    }
    // SET NULL or SET DEFAULT.
    if let Some(set) = s.eat_keyword("SET") {
        if s.ws1() {
            if let Some(null) = s.eat_keyword("NULL") {
                return Some(format!("{} {}", set, null));
            }
            if let Some(def) = s.eat_keyword("DEFAULT") {
                return Some(format!("{} {}", set, def));
            }
        }
        s.set(save);
    }
    // NO ACTION
    if let Some(no) = s.eat_keyword("NO") {
        if s.ws1() {
            if let Some(action) = s.eat_keyword("ACTION") {
                return Some(format!("{} {}", no, action));
            }
        }
        s.set(save);
    }
    None
}

/// Parse a datatype, requiring success. Used by column and add-column rules.
pub fn require_datatype(s: &mut Stream) -> Option<Value> {
    o_datatype(s)
}
