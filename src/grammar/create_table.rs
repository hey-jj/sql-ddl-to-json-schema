//! CREATE TABLE grammar rules.

use serde_json::{json, Map, Value};

use crate::lexer::TokenKind;

use super::common::{
    column_definitions, index_column_list, index_options, one_of_keywords, p_column_reference,
    p_index_type, require_datatype, ws_or_equals,
};
use super::helpers::{o_quoted_string, o_string_or_ident, s_identifier, s_number};
use super::stream::Stream;

/// Parse `P_CREATE_TABLE`. Tries the common form, then the LIKE form.
pub fn p_create_table(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if let Some(inner) = p_create_table_common(s) {
        return Some(json!({ "id": "P_CREATE_TABLE", "def": inner }));
    }
    s.set(save);
    if let Some(inner) = p_create_table_like(s) {
        return Some(json!({ "id": "P_CREATE_TABLE", "def": inner }));
    }
    s.set(save);
    None
}

/// Parse the leading `CREATE [OR REPLACE]? [TEMPORARY]? TABLE [IF NOT EXISTS]?`.
/// Returns true on success. Leaves the cursor after `TABLE` and the flags.
fn create_table_head(s: &mut Stream) -> bool {
    if s.eat_keyword("CREATE").is_none() {
        return false;
    }
    // OR REPLACE.
    let save = s.pos();
    if s.ws1() && s.eat_keyword("OR").is_some() && s.ws1() && s.eat_keyword("REPLACE").is_some() {
        // consumed
    } else {
        s.set(save);
    }
    // TEMPORARY.
    let save = s.pos();
    if s.ws1() && s.eat_keyword("TEMPORARY").is_some() {
        // consumed
    } else {
        s.set(save);
    }
    if !s.ws1() || s.eat_keyword("TABLE").is_none() {
        return false;
    }
    // IF NOT EXISTS.
    let save = s.pos();
    if s.ws1()
        && s.eat_keyword("IF").is_some()
        && s.ws1()
        && s.eat_keyword("NOT").is_some()
        && s.ws1()
        && s.eat_keyword("EXISTS").is_some()
    {
        // consumed
    } else {
        s.set(save);
    }
    true
}

fn p_create_table_common(s: &mut Stream) -> Option<Value> {
    if !create_table_head(s) {
        return None;
    }
    if !s.ws1() {
        return None;
    }
    let table = s_identifier(s)?;
    s.ws0();
    let columns_def = p_create_table_create_definitions(s)?;

    // Optional table options.
    let mut table_options: Option<Value> = None;
    let save = s.pos();
    s.ws0();
    if let Some(opts) = p_create_table_options(s) {
        table_options = Some(opts);
    } else {
        s.set(save);
    }

    // End of statement.
    if !s_eos(s) {
        return None;
    }

    let mut def = Map::new();
    def.insert("table".into(), Value::String(table));
    def.insert("columnsDef".into(), columns_def);
    // The source stores `tableOptions: d[10]`, which is null when no options
    // follow. The compact model reads it with a defined check, so null is safe.
    def.insert("tableOptions".into(), table_options.unwrap_or(Value::Null));
    Some(json!({ "id": "P_CREATE_TABLE_COMMON", "def": def }))
}

fn p_create_table_like(s: &mut Stream) -> Option<Value> {
    if !create_table_head(s) {
        return None;
    }
    if !s.ws1() {
        return None;
    }
    let table = s_identifier(s)?;

    // `__ LIKE __ ident` or `_ ( _ LIKE __ ident _ )`.
    let save = s.pos();
    let like;
    if s.ws1() && s.eat_keyword("LIKE").is_some() && s.ws1() {
        like = s_identifier(s)?;
    } else {
        s.set(save);
        s.ws0();
        s.eat(&TokenKind::LParens)?;
        s.ws0();
        if s.eat_keyword("LIKE").is_none() || !s.ws1() {
            return None;
        }
        like = s_identifier(s)?;
        s.ws0();
        s.eat(&TokenKind::RParens)?;
    }

    if !s_eos(s) {
        return None;
    }

    Some(json!({
        "id": "P_CREATE_TABLE_LIKE",
        "def": { "table": table, "like": like }
    }))
}

/// Parse `( def (, def)* )`.
fn p_create_table_create_definitions(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    s.eat(&TokenKind::LParens)?;
    s.ws0();
    let first = match o_create_table_create_definition(s) {
        Some(d) => d,
        None => {
            s.set(save);
            return None;
        }
    };
    let mut defs = vec![first];
    loop {
        let item = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Comma).is_none() {
            s.set(item);
            break;
        }
        s.ws0();
        match o_create_table_create_definition(s) {
            Some(d) => defs.push(d),
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
    Some(json!({
        "id": "P_CREATE_TABLE_CREATE_DEFINITIONS",
        "def": defs
    }))
}

/// Wrap a create definition alternative.
fn wrap_def(inner: Value) -> Value {
    json!({ "id": "O_CREATE_TABLE_CREATE_DEFINITION", "def": inner })
}

/// Parse one create definition. Tries index and key forms before columns so a
/// leading keyword like PRIMARY or INDEX is not read as a column name.
fn o_create_table_create_definition(s: &mut Stream) -> Option<Value> {
    let save = s.pos();

    if let Some(v) = def_primary_key(s) {
        return Some(wrap_def(v));
    }
    s.set(save);
    if let Some(v) = def_index(s) {
        return Some(wrap_def(v));
    }
    s.set(save);
    if let Some(v) = def_unique_key(s) {
        return Some(wrap_def(v));
    }
    s.set(save);
    if let Some(v) = def_fulltext(s) {
        return Some(wrap_def(v));
    }
    s.set(save);
    if let Some(v) = def_spatial(s) {
        return Some(wrap_def(v));
    }
    s.set(save);
    if let Some(v) = def_foreign_key(s) {
        return Some(wrap_def(v));
    }
    s.set(save);
    if let Some(v) = def_column(s) {
        return Some(wrap_def(v));
    }
    s.set(save);
    None
}

/// Optional `__ ident` after a keyword. Returns the identifier.
fn opt_ws_ident(s: &mut Stream) -> Option<String> {
    let save = s.pos();
    if s.ws1() {
        if let Some(id) = s_identifier(s) {
            return Some(id);
        }
    }
    s.set(save);
    None
}

/// Optional `__ P_INDEX_TYPE`. Returns the index type node.
fn opt_index_type(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.ws1() {
        if let Some(it) = p_index_type(s) {
            return Some(it);
        }
    }
    s.set(save);
    None
}

fn def_column(s: &mut Stream) -> Option<Value> {
    let name = s_identifier(s)?;
    s.ws0();
    let datatype = require_datatype(s)?;
    let column_definition = column_definitions(s);
    // Optional reference.
    let mut reference = None;
    let save = s.pos();
    if s.ws1() {
        if let Some(r) = p_column_reference(s) {
            reference = Some(r);
        } else {
            s.set(save);
        }
    } else {
        s.set(save);
    }

    let mut inner = Map::new();
    inner.insert("datatype".into(), datatype);
    inner.insert("columnDefinition".into(), Value::Array(column_definition));
    if let Some(r) = reference {
        inner.insert("reference".into(), r);
    }
    Some(json!({
        "column": { "name": name, "def": inner }
    }))
}

/// Track a constraint name that may be a present empty option vs absent.
struct Constraint(Option<String>);

/// Parse an optional constraint prefix followed by `next`, keeping the
/// present-but-unnamed case.
///
/// The name is kept only when `next` follows it, so a keyword like PRIMARY is
/// not swallowed as a constraint name.
fn opt_constraint_full(s: &mut Stream, next: &str) -> Constraint {
    let save = s.pos();
    if s.eat_keyword("CONSTRAINT").is_none() {
        s.set(save);
        return Constraint(None);
    }
    // Try with a name.
    let with_name = s.pos();
    if s.ws1() {
        if let Some(n) = s_identifier(s) {
            if s.ws1() && peek_keyword(s, next) {
                return Constraint(Some(n));
            }
        }
    }
    // Retry without a name.
    s.set(with_name);
    if !s.ws1() {
        s.set(save);
        return Constraint(None);
    }
    Constraint(Some(String::new()))
}

/// Whether the next token is the given keyword, without consuming it.
fn peek_keyword(s: &Stream, name: &str) -> bool {
    matches!(
        s.peek(),
        Some(crate::lexer::Token { kind: crate::lexer::TokenKind::Keyword(k), .. }) if k == name
    )
}

/// Convert a constraint name into a JSON value: undefined (absent) or a string.
fn constraint_name(c: Constraint) -> Value {
    match c.0 {
        Some(n) if !n.is_empty() => Value::String(n),
        // Absent, or present but unnamed: the source yields undefined here.
        _ => Value::Null,
    }
}

fn def_primary_key(s: &mut Stream) -> Option<Value> {
    let constraint = opt_constraint_full(s, "PRIMARY");
    s.eat_keyword("PRIMARY")?;
    if !s.ws1() || s.eat_keyword("KEY").is_none() {
        return None;
    }
    let index = opt_index_type(s);
    let columns = index_column_list(s)?;
    let options = index_options(s);

    let mut pk = Map::new();
    // Both name (constraint) and index (index type) are null when absent.
    pk.insert("name".into(), constraint_name(constraint));
    pk.insert("index".into(), index.unwrap_or(Value::Null));
    pk.insert("columns".into(), Value::Array(columns));
    pk.insert("options".into(), Value::Array(options));
    Some(json!({ "primaryKey": pk }))
}

fn def_index(s: &mut Stream) -> Option<Value> {
    one_of_keywords(s, &["INDEX", "KEY"])?;
    let name = opt_ws_ident(s);
    let index = opt_index_type(s);
    let columns = index_column_list(s)?;
    let options = index_options(s);

    let mut idx = Map::new();
    idx.insert(
        "name".into(),
        name.map(Value::String).unwrap_or(Value::Null),
    );
    idx.insert("index".into(), index.unwrap_or(Value::Null));
    idx.insert("columns".into(), Value::Array(columns));
    idx.insert("options".into(), Value::Array(options));
    Some(json!({ "index": idx }))
}

fn def_unique_key(s: &mut Stream) -> Option<Value> {
    // The constraint prefix is consumed but its name is not used here. The
    // unique key name comes from the index identifier after UNIQUE.
    let _constraint = opt_constraint_full(s, "UNIQUE");
    s.eat_keyword("UNIQUE")?;
    // The `( __ INDEX | __ KEY )?` optional prefers its absent branch, so the
    // word index or key is read as the identifier first and the parse only
    // consumes the keyword branch when the identifier branch cannot continue.
    let (name, index, columns, options) = match unique_key_tail(s, false) {
        Some(t) => t,
        None => unique_key_tail(s, true)?,
    };

    let mut uk = Map::new();
    // Name is `d[3]`, null when absent. The workaround sets it to undefined
    // (omitted) when it parsed as the literal word index or key.
    match name {
        Some(n) if is_index_or_key(&n) => {}
        Some(n) => {
            uk.insert("name".into(), Value::String(n));
        }
        None => {
            uk.insert("name".into(), Value::Null);
        }
    }
    // The source sets `index: d[4] ?? undefined`, so absent means omitted.
    insert_opt(&mut uk, "index", index);
    uk.insert("columns".into(), Value::Array(columns));
    uk.insert("options".into(), Value::Array(options));
    Some(json!({ "uniqueKey": uk }))
}

/// Parse the tail of a unique key after UNIQUE: an optional INDEX/KEY branch, a
/// name, an index type, columns, and options.
///
/// `consume_kw` controls whether the `__ INDEX | __ KEY` branch is taken.
type UniqueTail = (Option<String>, Option<Value>, Vec<Value>, Vec<Value>);

fn unique_key_tail(s: &mut Stream, consume_kw: bool) -> Option<UniqueTail> {
    let save = s.pos();
    if consume_kw {
        let ksave = s.pos();
        if s.ws1() && one_of_keywords(s, &["INDEX", "KEY"]).is_some() {
        } else {
            s.set(ksave);
            return None;
        }
    }
    let name = opt_ws_ident(s);
    let index = opt_index_type(s);
    let columns = match index_column_list(s) {
        Some(c) => c,
        None => {
            s.set(save);
            return None;
        }
    };
    let options = index_options(s);
    Some((name, index, columns, options))
}

fn def_fulltext(s: &mut Stream) -> Option<Value> {
    s.eat_keyword("FULLTEXT")?;
    let save = s.pos();
    if s.ws1() && one_of_keywords(s, &["INDEX", "KEY"]).is_some() {
        // consumed
    } else {
        s.set(save);
    }
    let name = opt_ws_ident(s);
    let columns = index_column_list(s)?;
    let options = index_options(s);

    let mut fi = Map::new();
    match name {
        Some(n) if is_index_or_key(&n) => {}
        Some(n) => {
            fi.insert("name".into(), Value::String(n));
        }
        None => {
            fi.insert("name".into(), Value::Null);
        }
    }
    fi.insert("columns".into(), Value::Array(columns));
    fi.insert("options".into(), Value::Array(options));
    Some(json!({ "fulltextIndex": fi }))
}

fn def_spatial(s: &mut Stream) -> Option<Value> {
    s.eat_keyword("SPATIAL")?;
    let save = s.pos();
    if s.ws1() && one_of_keywords(s, &["INDEX", "KEY"]).is_some() {
        // consumed
    } else {
        s.set(save);
    }
    let name = opt_ws_ident(s);
    let columns = index_column_list(s)?;
    let options = index_options(s);

    let mut si = Map::new();
    match name {
        Some(n) if is_index_or_key(&n) => {}
        Some(n) => {
            si.insert("name".into(), Value::String(n));
        }
        None => {
            si.insert("name".into(), Value::Null);
        }
    }
    si.insert("columns".into(), Value::Array(columns));
    si.insert("options".into(), Value::Array(options));
    Some(json!({ "spatialIndex": si }))
}

fn def_foreign_key(s: &mut Stream) -> Option<Value> {
    let constraint = opt_constraint_full(s, "FOREIGN");
    s.eat_keyword("FOREIGN")?;
    if !s.ws1() || s.eat_keyword("KEY").is_none() {
        return None;
    }
    // A named index after FOREIGN KEY is parsed but not used for the name. The
    // name comes from the constraint prefix (null when absent).
    let _index_name = opt_ws_ident(s);
    let columns = index_column_list(s)?;
    s.ws0();
    let reference = p_column_reference(s)?;

    let mut fk = Map::new();
    fk.insert("name".into(), constraint_name(constraint));
    fk.insert("columns".into(), Value::Array(columns));
    fk.insert("reference".into(), reference);
    Some(json!({ "foreignKey": fk }))
}

/// Insert a value only if it is not null.
fn insert_opt(map: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(v) = value {
        map.insert(key.into(), v);
    }
}

/// Whether a name is the literal word index or key, case-insensitively.
fn is_index_or_key(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "index" || lower == "key"
}

/// Parse `S_EOS`: optional whitespace then a semicolon.
pub fn s_eos(s: &mut Stream) -> bool {
    let save = s.pos();
    s.ws0();
    if s.eat(&TokenKind::Semicolon).is_some() {
        true
    } else {
        s.set(save);
        false
    }
}

/// Parse `P_CREATE_TABLE_OPTIONS`: options separated by whitespace or comma.
pub fn p_create_table_options(s: &mut Stream) -> Option<Value> {
    let first = o_create_table_option(s)?;
    let mut opts = vec![first];
    loop {
        let save = s.pos();
        // Separator: `( __ | _ , _ )`.
        let sep_ok = {
            let ws_save = s.pos();
            if s.ws1() {
                true
            } else {
                s.set(ws_save);
                s.ws0();
                if s.eat(&TokenKind::Comma).is_some() {
                    s.ws0();
                    true
                } else {
                    false
                }
            }
        };
        if !sep_ok {
            s.set(save);
            break;
        }
        match o_create_table_option(s) {
            Some(o) => opts.push(o),
            None => {
                s.set(save);
                break;
            }
        }
    }
    Some(json!({ "id": "P_CREATE_TABLE_OPTIONS", "def": opts }))
}

/// Wrap a table option alternative.
fn wrap_opt(def: Value) -> Value {
    json!({ "id": "O_CREATE_TABLE_OPTION", "def": def })
}

/// Parse one table option.
fn o_create_table_option(s: &mut Stream) -> Option<Value> {
    let save = s.pos();

    // Numeric options: keyword ( __ | = ) NUMBER.
    let numeric: &[(&str, &str)] = &[
        ("AUTO_INCREMENT", "autoincrement"),
        ("AVG_ROW_LENGTH", "avgRowLength"),
        ("CHECKSUM", "checksum"),
        ("DELAY_KEY_WRITE", "delayKeyWrite"),
        ("ENCRYPTION_KEY_ID", "encryptionKeyId"),
        ("KEY_BLOCK_SIZE", "keyBlockSize"),
        ("MAX_ROWS", "maxRows"),
        ("MIN_ROWS", "minRows"),
        ("PAGE_CHECKSUM", "pageChecksum"),
        ("TRANSACTIONAL", "transactional"),
    ];
    for (kw, field) in numeric {
        if s.eat_keyword(kw).is_some() {
            if ws_or_equals(s) {
                if let Some(n) = s_number(s) {
                    return Some(wrap_opt(json!({ *field: n })));
                }
            }
            s.set(save);
        }
    }

    // [DEFAULT]? (CHARACTER SET | CHARSET) ( __ | = ) charset
    if let Some(v) = opt_charset(s) {
        return Some(wrap_opt(json!({ "charset": v })));
    }
    s.set(save);

    // [DEFAULT]? COLLATE ( __ | = ) collation
    if let Some(v) = opt_collate(s) {
        return Some(wrap_opt(json!({ "collation": v })));
    }
    s.set(save);

    // Quoted-string options.
    let quoted: &[(&str, &str)] = &[
        ("COMMENT", "comment"),
        ("COMPRESSION", "compression"),
        ("CONNECTION", "connection"),
        ("ENCRYPTION", "encryption"),
        ("PASSWORD", "password"),
    ];
    for (kw, field) in quoted {
        if s.eat_keyword(kw).is_some() {
            if ws_or_equals(s) {
                if let Some(v) = o_quoted_string(s) {
                    return Some(wrap_opt(json!({ *field: v })));
                }
            }
            s.set(save);
        }
    }

    // (DATA|INDEX) DIRECTORY ( __ | = ) string
    if let Some(v) = opt_directory(s) {
        return Some(wrap_opt(v));
    }
    s.set(save);

    // IETF_QUOTES ( __ | = ) (YES|NO)
    if s.eat_keyword("IETF_QUOTES").is_some() {
        if ws_or_equals(s) {
            if let Some(v) = one_of_keywords(s, &["YES", "NO"]) {
                return Some(wrap_opt(json!({ "ietfQuotes": v })));
            }
        }
        s.set(save);
    }

    // ENGINE ( __ | = ) engine
    if s.eat_keyword("ENGINE").is_some() {
        if ws_or_equals(s) {
            if let Some(v) = o_string_or_ident(s) {
                return Some(wrap_opt(json!({ "engine": v })));
            }
        }
        s.set(save);
    }

    // INSERT_METHOD ( __ | = ) (NO|FIRST|LAST)
    if s.eat_keyword("INSERT_METHOD").is_some() {
        if ws_or_equals(s) {
            if let Some(v) = one_of_keywords(s, &["NO", "FIRST", "LAST"]) {
                return Some(wrap_opt(json!({ "insertMethod": v })));
            }
        }
        s.set(save);
    }

    // PACK_KEYS ( __ | = ) (NUMBER|DEFAULT)
    if s.eat_keyword("PACK_KEYS").is_some() {
        if ws_or_equals(s) {
            if let Some(v) = number_or_default(s) {
                return Some(wrap_opt(json!({ "packKeys": v })));
            }
        }
        s.set(save);
    }

    // ROW_FORMAT ( __ | = ) (...)
    if s.eat_keyword("ROW_FORMAT").is_some() {
        if ws_or_equals(s) {
            if let Some(v) = one_of_keywords(
                s,
                &[
                    "DEFAULT",
                    "DYNAMIC",
                    "FIXED",
                    "COMPRESSED",
                    "REDUNDANT",
                    "COMPACT",
                    "PAGE",
                ],
            ) {
                return Some(wrap_opt(json!({ "rowFormat": v })));
            }
        }
        s.set(save);
    }

    // STATS_AUTO_RECALC / STATS_PERSISTENT ( __ | = ) (NUMBER|DEFAULT)
    for (kw, field) in [
        ("STATS_AUTO_RECALC", "statsAutoRecalc"),
        ("STATS_PERSISTENT", "statsPersistent"),
    ] {
        if s.eat_keyword(kw).is_some() {
            if ws_or_equals(s) {
                if let Some(v) = number_or_default(s) {
                    return Some(wrap_opt(json!({ field: v })));
                }
            }
            s.set(save);
        }
    }

    // STATS_SAMPLE_PAGES ( __ | = ) value
    if s.eat_keyword("STATS_SAMPLE_PAGES").is_some() {
        if ws_or_equals(s) {
            if let Some(v) = super::helpers::o_table_option_value(s) {
                return Some(wrap_opt(json!({ "statsSamplePages": v })));
            }
        }
        s.set(save);
    }

    // WITH SYSTEM VERSIONING
    if s.eat_keyword("WITH").is_some() {
        if s.ws1()
            && s.eat_keyword("SYSTEM").is_some()
            && s.ws1()
            && s.eat_keyword("VERSIONING").is_some()
        {
            return Some(wrap_opt(json!({ "withSystemVersioning": true })));
        }
        s.set(save);
    }

    // TABLESPACE ident [STORAGE ...]?
    if s.eat_keyword("TABLESPACE").is_some() {
        if s.ws1() {
            if let Some(name) = s_identifier(s) {
                let mut obj = Map::new();
                obj.insert("tablespaceName".into(), Value::String(name));
                let stsave = s.pos();
                if s.ws1() && s.eat_keyword("STORAGE").is_some() && s.ws1() {
                    if let Some(v) = one_of_keywords(s, &["DISK", "MEMORY", "DEFAULT"]) {
                        obj.insert("tablespaceStorage".into(), Value::String(v));
                    } else {
                        s.set(stsave);
                    }
                } else {
                    s.set(stsave);
                }
                return Some(wrap_opt(Value::Object(obj)));
            }
        }
        s.set(save);
    }

    // UNION ( __ | = ) ( idents )
    if s.eat_keyword("UNION").is_some() {
        if ws_or_equals(s) {
            if let Some(list) = ident_list_parens(s) {
                return Some(wrap_opt(json!({ "union": list })));
            }
        }
        s.set(save);
    }

    None
}

/// Parse `[DEFAULT]? (CHARACTER SET | CHARSET) ( __ | = ) charset`.
fn opt_charset(s: &mut Stream) -> Option<String> {
    let save = s.pos();
    // Optional DEFAULT.
    let dsave = s.pos();
    if s.eat_keyword("DEFAULT").is_some() && s.ws1() {
        // consumed
    } else {
        s.set(dsave);
    }
    // CHARACTER SET | CHARSET.
    let matched = {
        let msave = s.pos();
        if s.eat_keyword("CHARACTER").is_some() && s.ws1() && s.eat_keyword("SET").is_some() {
            true
        } else {
            s.set(msave);
            s.eat_keyword("CHARSET").is_some()
        }
    };
    if !matched {
        s.set(save);
        return None;
    }
    if !ws_or_equals(s) {
        s.set(save);
        return None;
    }
    match o_string_or_ident(s) {
        Some(v) => Some(v),
        None => {
            s.set(save);
            None
        }
    }
}

/// Parse `[DEFAULT]? COLLATE ( __ | = ) collation`.
fn opt_collate(s: &mut Stream) -> Option<String> {
    let save = s.pos();
    let dsave = s.pos();
    if s.eat_keyword("DEFAULT").is_some() && s.ws1() {
        // consumed
    } else {
        s.set(dsave);
    }
    if s.eat_keyword("COLLATE").is_none() {
        s.set(save);
        return None;
    }
    if !ws_or_equals(s) {
        s.set(save);
        return None;
    }
    match o_string_or_ident(s) {
        Some(v) => Some(v),
        None => {
            s.set(save);
            None
        }
    }
}

/// Parse `(DATA|INDEX) DIRECTORY ( __ | = ) string`.
fn opt_directory(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    let kind = {
        let dsave = s.pos();
        if let Some(v) = s.eat_keyword("DATA") {
            if s.ws1() {
                v
            } else {
                s.set(dsave);
                return None;
            }
        } else if let Some(v) = s.eat_keyword("INDEX") {
            if s.ws1() {
                v
            } else {
                s.set(dsave);
                return None;
            }
        } else {
            return None;
        }
    };
    if s.eat_keyword("DIRECTORY").is_none() {
        s.set(save);
        return None;
    }
    if !ws_or_equals(s) {
        s.set(save);
        return None;
    }
    match o_quoted_string(s) {
        Some(v) => {
            let key = format!("{}Directory", kind.to_lowercase());
            let mut obj = Map::new();
            obj.insert(key, Value::String(v));
            Some(Value::Object(obj))
        }
        None => {
            s.set(save);
            None
        }
    }
}

/// Parse `NUMBER | DEFAULT`. Numbers stay numeric, DEFAULT stays a string.
fn number_or_default(s: &mut Stream) -> Option<Value> {
    if let Some(n) = s_number(s) {
        return Some(n);
    }
    s.eat_keyword("DEFAULT").map(Value::String)
}

/// Parse `( ident (, ident)* )`.
fn ident_list_parens(s: &mut Stream) -> Option<Vec<String>> {
    let save = s.pos();
    s.eat(&TokenKind::LParens)?;
    s.ws0();
    let first = match s_identifier(s) {
        Some(v) => v,
        None => {
            s.set(save);
            return None;
        }
    };
    let mut list = vec![first];
    loop {
        let item = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Comma).is_none() {
            s.set(item);
            break;
        }
        s.ws0();
        match s_identifier(s) {
            Some(v) => list.push(v),
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
    Some(list)
}
