//! Remaining statement rules: create index, databases, drop, rename, set, use.

use serde_json::{json, Map, Value};

use crate::lexer::TokenKind;

use super::common::{
    index_column_list, index_options, one_of_keywords, p_index_algorithm_option, p_index_type,
    p_lock_option, ws_or_equals,
};
use super::create_table::s_eos;
use super::helpers::{o_string_or_ident, s_identifier, s_number};
use super::stream::Stream;

/// Parse `P_CREATE_INDEX`.
pub fn p_create_index(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    s.eat_keyword("CREATE")?;
    // OR REPLACE.
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("OR").is_some() && s.ws1() && s.eat_keyword("REPLACE").is_some() {
    } else {
        s.set(sp);
    }
    // ONLINE | OFFLINE.
    let sp = s.pos();
    if s.ws1() && one_of_keywords(s, &["ONLINE", "OFFLINE"]).is_some() {
    } else {
        s.set(sp);
    }
    // UNIQUE | FULLTEXT | SPATIAL.
    let mut modifier: Option<String> = None;
    let sp = s.pos();
    if s.ws1() {
        if let Some(v) = one_of_keywords(s, &["UNIQUE", "FULLTEXT", "SPATIAL"]) {
            modifier = Some(v);
        } else {
            s.set(sp);
        }
    } else {
        s.set(sp);
    }
    if !s.ws1() {
        s.set(save);
        return None;
    }
    let index_kw = match s.eat_keyword("INDEX") {
        Some(v) => v,
        None => {
            s.set(save);
            return None;
        }
    };
    // IF NOT EXISTS.
    let sp = s.pos();
    if s.ws1()
        && s.eat_keyword("IF").is_some()
        && s.ws1()
        && s.eat_keyword("NOT").is_some()
        && s.ws1()
        && s.eat_keyword("EXISTS").is_some()
    {
    } else {
        s.set(sp);
    }
    if !s.ws1() {
        s.set(save);
        return None;
    }
    let name = match s_identifier(s) {
        Some(n) => n,
        None => {
            s.set(save);
            return None;
        }
    };
    // Optional index type.
    let mut index = None;
    let sp = s.pos();
    if s.ws1() {
        if let Some(it) = p_index_type(s) {
            index = Some(it);
        } else {
            s.set(sp);
        }
    } else {
        s.set(sp);
    }
    if !s.ws1() || s.eat_keyword("ON").is_none() || !s.ws1() {
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
    // Optional column list.
    let mut columns: Option<Vec<Value>> = None;
    let sp = s.pos();
    if let Some(cols) = index_column_list(s) {
        columns = Some(cols);
    } else {
        s.set(sp);
    }
    // Optional WAIT n | NOWAIT.
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("WAIT").is_some() && s.ws1() && s_number(s).is_some() {
    } else {
        s.set(sp);
        let sp2 = s.pos();
        if s.ws1() && s.eat_keyword("NOWAIT").is_some() {
        } else {
            s.set(sp2);
        }
    }
    // Index options.
    let idx_opts = index_options(s);
    // Algorithm and lock options.
    let mut alg_lock = Vec::new();
    loop {
        let sp = s.pos();
        s.ws0();
        if let Some(a) = p_index_algorithm_option(s) {
            alg_lock.push(a);
            continue;
        }
        s.set(sp);
        s.ws0();
        if let Some(l) = p_lock_option(s) {
            alg_lock.push(l);
            continue;
        }
        s.set(sp);
        break;
    }

    if !s_eos(s) {
        s.set(save);
        return None;
    }

    // type = (modifier value + ' ')? + INDEX value, using the raw matched text.
    let type_str = match modifier {
        Some(m) => format!("{} {}", m, index_kw),
        None => index_kw,
    };

    let mut options = idx_opts;
    options.extend(alg_lock);

    let mut def = Map::new();
    def.insert("name".into(), Value::String(name));
    def.insert("type".into(), Value::String(type_str));
    // index (index type) is null when absent.
    def.insert("index".into(), index.unwrap_or(Value::Null));
    def.insert("table".into(), Value::String(table));
    // columns may be undefined; source stores d[14] which can be undefined.
    if let Some(c) = columns {
        def.insert("columns".into(), Value::Array(c));
    }
    def.insert("options".into(), Value::Array(options));
    Some(json!({ "id": "P_CREATE_INDEX", "def": def }))
}

/// Parse `O_CREATE_DB_SPEC` / `O_ALTER_DB_SPEC`.
fn db_spec(s: &mut Stream, id: &str) -> Option<Value> {
    let save = s.pos();
    // [DEFAULT]? (CHARACTER SET | CHARSET) ( __ | = ) charset
    if let Some(v) = db_charset(s) {
        return Some(json!({ "id": id, "def": { "charset": v } }));
    }
    s.set(save);
    // [DEFAULT]? COLLATE ( __ | = ) collation
    if let Some(v) = db_collate(s) {
        return Some(json!({ "id": id, "def": { "collation": v } }));
    }
    s.set(save);
    None
}

fn db_charset(s: &mut Stream) -> Option<String> {
    let save = s.pos();
    let dsave = s.pos();
    if s.eat_keyword("DEFAULT").is_some() && s.ws1() {
    } else {
        s.set(dsave);
    }
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

fn db_collate(s: &mut Stream) -> Option<String> {
    let save = s.pos();
    let dsave = s.pos();
    if s.eat_keyword("DEFAULT").is_some() && s.ws1() {
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

/// Parse `P_CREATE_DB`.
pub fn p_create_db(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.eat_keyword("CREATE").is_none() || !s.ws1() {
        return None;
    }
    // OR REPLACE __.
    let sp = s.pos();
    if s.eat_keyword("OR").is_some() && s.ws1() && s.eat_keyword("REPLACE").is_some() && s.ws1() {
    } else {
        s.set(sp);
    }
    if one_of_keywords(s, &["DATABASE", "SCHEMA"]).is_none() {
        s.set(save);
        return None;
    }
    // IF [NOT]? EXISTS.
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("IF").is_some() {
        let nsp = s.pos();
        if s.ws1() && s.eat_keyword("NOT").is_some() {
        } else {
            s.set(nsp);
        }
        if s.ws1() && s.eat_keyword("EXISTS").is_some() {
        } else {
            s.set(sp);
        }
    } else {
        s.set(sp);
    }
    if !s.ws1() {
        s.set(save);
        return None;
    }
    let database = match s_identifier(s) {
        Some(d) => d,
        None => {
            s.set(save);
            return None;
        }
    };
    // meta specs.
    let mut meta = Vec::new();
    loop {
        let sp = s.pos();
        if s.ws1() {
            if let Some(spec) = db_spec(s, "O_CREATE_DB_SPEC") {
                meta.push(spec);
                continue;
            }
        }
        s.set(sp);
        break;
    }
    if !s_eos(s) {
        s.set(save);
        return None;
    }
    Some(json!({
        "id": "P_CREATE_DB",
        "def": { "database": database, "meta": meta }
    }))
}

/// Parse `P_ALTER_DB`.
pub fn p_alter_db(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.eat_keyword("ALTER").is_none() || !s.ws1() {
        return None;
    }
    if one_of_keywords(s, &["DATABASE", "SCHEMA"]).is_none() {
        s.set(save);
        return None;
    }
    // Optional database name.
    let mut database: Option<String> = None;
    let sp = s.pos();
    if s.ws1() {
        if let Some(d) = s_identifier(s) {
            database = Some(d);
        } else {
            s.set(sp);
        }
    } else {
        s.set(sp);
    }
    // One or more specs.
    let mut meta = Vec::new();
    loop {
        let sp = s.pos();
        if s.ws1() {
            if let Some(spec) = db_spec(s, "O_ALTER_DB_SPEC") {
                meta.push(spec);
                continue;
            }
        }
        s.set(sp);
        break;
    }
    if meta.is_empty() {
        s.set(save);
        return None;
    }
    if !s_eos(s) {
        s.set(save);
        return None;
    }
    let mut def = Map::new();
    // database may be undefined.
    if let Some(d) = database {
        def.insert("database".into(), Value::String(d));
    }
    def.insert("meta".into(), Value::Array(meta));
    Some(json!({ "id": "P_ALTER_DB", "def": def }))
}

/// Parse `P_DROP_DB`.
pub fn p_drop_db(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.eat_keyword("DROP").is_none() || !s.ws1() {
        return None;
    }
    if one_of_keywords(s, &["DATABASE", "SCHEMA"]).is_none() {
        s.set(save);
        return None;
    }
    // IF EXISTS.
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("IF").is_some() && s.ws1() && s.eat_keyword("EXISTS").is_some() {
    } else {
        s.set(sp);
    }
    if !s.ws1() {
        s.set(save);
        return None;
    }
    let db = match s_identifier(s) {
        Some(d) => d,
        None => {
            s.set(save);
            return None;
        }
    };
    if !s_eos(s) {
        s.set(save);
        return None;
    }
    Some(json!({ "id": "P_DROP_DB", "def": db }))
}

/// Parse `P_DROP_INDEX`.
pub fn p_drop_index(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.eat_keyword("DROP").is_none() || !s.ws1() {
        return None;
    }
    // ONLINE __ | OFFLINE __.
    let sp = s.pos();
    if (s.eat_keyword("ONLINE").is_some() || s.eat_keyword("OFFLINE").is_some()) && s.ws1() {
    } else {
        s.set(sp);
    }
    if s.eat_keyword("INDEX").is_none() {
        s.set(save);
        return None;
    }
    // IF EXISTS.
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("IF").is_some() && s.ws1() && s.eat_keyword("EXISTS").is_some() {
    } else {
        s.set(sp);
    }
    if !s.ws1() {
        s.set(save);
        return None;
    }
    let index = match s_identifier(s) {
        Some(i) => i,
        None => {
            s.set(save);
            return None;
        }
    };
    if !s.ws1() || s.eat_keyword("ON").is_none() || !s.ws1() {
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
    // WAIT n | NOWAIT.
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("WAIT").is_some() && s.ws1() && s_number(s).is_some() {
    } else {
        s.set(sp);
        let sp2 = s.pos();
        if s.ws1() && s.eat_keyword("NOWAIT").is_some() {
        } else {
            s.set(sp2);
        }
    }
    // Algorithm and lock options.
    let mut options = Vec::new();
    loop {
        let sp = s.pos();
        if s.ws1() {
            if let Some(a) = p_index_algorithm_option(s) {
                options.push(a);
                continue;
            }
            s.set(sp);
            if s.ws1() {
                if let Some(l) = p_lock_option(s) {
                    options.push(l);
                    continue;
                }
            }
        }
        s.set(sp);
        break;
    }
    if !s_eos(s) {
        s.set(save);
        return None;
    }
    Some(json!({
        "id": "P_DROP_INDEX",
        "def": { "index": index, "table": table, "options": options }
    }))
}

/// Parse `P_DROP_TABLE`.
pub fn p_drop_table(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.eat_keyword("DROP").is_none() || !s.ws1() {
        return None;
    }
    // TEMPORARY __.
    let sp = s.pos();
    if s.eat_keyword("TEMPORARY").is_some() && s.ws1() {
    } else {
        s.set(sp);
    }
    if s.eat_keyword("TABLE").is_none() {
        s.set(save);
        return None;
    }
    // IF EXISTS.
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("IF").is_some() && s.ws1() && s.eat_keyword("EXISTS").is_some() {
    } else {
        s.set(sp);
    }
    if !s.ws1() {
        s.set(save);
        return None;
    }
    let first = match s_identifier(s) {
        Some(t) => t,
        None => {
            s.set(save);
            return None;
        }
    };
    let mut tables = vec![Value::String(first)];
    loop {
        let sp = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Comma).is_none() {
            s.set(sp);
            break;
        }
        s.ws0();
        match s_identifier(s) {
            Some(t) => tables.push(Value::String(t)),
            None => {
                s.set(sp);
                break;
            }
        }
    }
    // WAIT n | NOWAIT.
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("WAIT").is_some() && s.ws1() && s_number(s).is_some() {
    } else {
        s.set(sp);
        let sp2 = s.pos();
        if s.ws1() && s.eat_keyword("NOWAIT").is_some() {
        } else {
            s.set(sp2);
        }
    }
    // RESTRICT | CASCADE.
    let sp = s.pos();
    if s.ws1() && one_of_keywords(s, &["RESTRICT", "CASCADE"]).is_some() {
    } else {
        s.set(sp);
    }
    if !s_eos(s) {
        s.set(save);
        return None;
    }
    Some(json!({ "id": "P_DROP_TABLE", "def": tables }))
}

/// Parse `P_RENAME_TABLE`.
pub fn p_rename_table(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.eat_keyword("RENAME").is_none() || !s.ws1() || s.eat_keyword("TABLE").is_none() {
        return None;
    }
    if !s.ws1() {
        s.set(save);
        return None;
    }
    let first_table = match s_identifier(s) {
        Some(t) => t,
        None => {
            s.set(save);
            return None;
        }
    };
    // WAIT n | NOWAIT.
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("WAIT").is_some() && s.ws1() && s_number(s).is_some() {
    } else {
        s.set(sp);
        let sp2 = s.pos();
        if s.ws1() && s.eat_keyword("NOWAIT").is_some() {
        } else {
            s.set(sp2);
        }
    }
    if !s.ws1() || s.eat_keyword("TO").is_none() || !s.ws1() {
        s.set(save);
        return None;
    }
    let first_new = match s_identifier(s) {
        Some(t) => t,
        None => {
            s.set(save);
            return None;
        }
    };
    let mut pairs = vec![json!({ "table": first_table, "newName": first_new })];
    loop {
        let sp = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Comma).is_none() {
            s.set(sp);
            break;
        }
        s.ws0();
        let table = match s_identifier(s) {
            Some(t) => t,
            None => {
                s.set(sp);
                break;
            }
        };
        if !s.ws1() || s.eat_keyword("TO").is_none() || !s.ws1() {
            s.set(sp);
            break;
        }
        let new_name = match s_identifier(s) {
            Some(t) => t,
            None => {
                s.set(sp);
                break;
            }
        };
        pairs.push(json!({ "table": table, "newName": new_name }));
    }
    if !s_eos(s) {
        s.set(save);
        return None;
    }
    Some(json!({ "id": "P_RENAME_TABLE", "def": pairs }))
}

/// Parse `P_SET`: `SET` then a run of any tokens, ending at `;`.
pub fn p_set(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.eat_keyword("SET").is_none() || !s.ws1() {
        return None;
    }
    let mut consumed_any = false;
    loop {
        // End when the next non-token boundary is `_ ;`.
        let sp = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Semicolon).is_some() {
            if !consumed_any {
                s.set(save);
                return None;
            }
            return Some(json!({ "id": "P_SET" }));
        }
        s.set(sp);
        // Consume one allowed token.
        match s.peek().map(|t| t.kind.clone()) {
            Some(kind) => match kind {
                TokenKind::Ws => {
                    s.bump();
                    // whitespace alone does not count as a set body token
                }
                TokenKind::Unknown
                | TokenKind::IdentifierUnquoted
                | TokenKind::IdentifierQuoted
                | TokenKind::Keyword(_)
                | TokenKind::Equal
                | TokenKind::LParens
                | TokenKind::RParens
                | TokenKind::Comma
                | TokenKind::BitFormat
                | TokenKind::HexaFormat
                | TokenKind::DQuoteString
                | TokenKind::SQuoteString
                | TokenKind::Number => {
                    s.bump();
                    consumed_any = true;
                }
                TokenKind::Semicolon => {
                    // Semicolon handled above.
                    s.set(save);
                    return None;
                }
            },
            None => {
                s.set(save);
                return None;
            }
        }
    }
}

/// Parse `P_USE_DB`.
pub fn p_use_db(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.eat_keyword("USE").is_none() || !s.ws1() {
        return None;
    }
    let db = match s_identifier(s) {
        Some(d) => d,
        None => {
            s.set(save);
            return None;
        }
    };
    if !s_eos(s) {
        s.set(save);
        return None;
    }
    Some(json!({
        "id": "P_USE_DB",
        "def": { "database": db }
    }))
}

#[cfg(test)]
mod tests {
    use super::super::parse_statement;

    #[test]
    fn set_accepts_quoted_string_assignment() {
        let parsed = parse_statement("SET sql_mode='ANSI';").unwrap();

        assert_eq!(parsed["def"]["id"], "P_SET");
    }
}
