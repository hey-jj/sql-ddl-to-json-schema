//! ALTER TABLE grammar rules.

use serde_json::{json, Map, Value};

use crate::lexer::TokenKind;

use super::common::{
    column_definitions, index_column_list, index_options, o_default_value, one_of_keywords,
    p_column_reference, p_index_type, require_datatype, ws_or_equals,
};
use super::create_table::{p_create_table_options, s_eos};
use super::helpers::{s_identifier, s_number};
use super::stream::Stream;

/// Parse `P_ALTER_TABLE`.
pub fn p_alter_table(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.eat_keyword("ALTER").is_none() || !s.ws1() {
        return None;
    }
    // ONLINE __.
    let sp = s.pos();
    if s.eat_keyword("ONLINE").is_some() && s.ws1() {
    } else {
        s.set(sp);
    }
    // IGNORE __.
    let sp = s.pos();
    if s.eat_keyword("IGNORE").is_some() && s.ws1() {
    } else {
        s.set(sp);
    }
    if s.eat_keyword("TABLE").is_none() || !s.ws1() {
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
    if !s.ws1() {
        s.set(save);
        return None;
    }
    // WAIT n __ | NOWAIT __.
    let sp = s.pos();
    if s.eat_keyword("WAIT").is_some() && s.ws1() && s_number(s).is_some() && s.ws1() {
    } else {
        s.set(sp);
        let sp2 = s.pos();
        if s.eat_keyword("NOWAIT").is_some() && s.ws1() {
        } else {
            s.set(sp2);
        }
    }
    let first = p_alter_table_specs(s)?;
    let mut specs = vec![first];
    loop {
        let sp = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Comma).is_none() {
            s.set(sp);
            break;
        }
        s.ws0();
        match p_alter_table_specs(s) {
            Some(spec) => specs.push(spec),
            None => {
                s.set(sp);
                break;
            }
        }
    }
    if !s_eos(s) {
        s.set(save);
        return None;
    }
    Some(json!({
        "id": "P_ALTER_TABLE",
        "def": { "table": table, "specs": specs }
    }))
}

/// Parse `P_ALTER_TABLE_SPECS`. Tries table options, then a spec.
fn p_alter_table_specs(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    // Table options first, matching source alternative order.
    if let Some(opts) = p_create_table_options(s) {
        return Some(json!({
            "id": "P_ALTER_TABLE_SPECS",
            "def": { "tableOptions": opts }
        }));
    }
    s.set(save);
    if let Some(spec) = o_alter_table_spec(s) {
        return Some(json!({
            "id": "P_ALTER_TABLE_SPECS",
            "def": { "spec": spec }
        }));
    }
    s.set(save);
    None
}

/// Wrap an alter spec alternative.
fn wrap(def: Value) -> Value {
    json!({ "id": "O_ALTER_TABLE_SPEC", "def": def })
}

/// Parse `O_ALTER_TABLE_SPEC`. Tries alternatives in source order.
fn o_alter_table_spec(s: &mut Stream) -> Option<Value> {
    let rules: &[fn(&mut Stream) -> Option<Value>] = &[
        add_column,
        add_columns,
        add_index,
        add_primary_key,
        add_unique_key,
        add_fulltext,
        add_spatial,
        add_foreign_key,
        algorithm,
        set_default,
        drop_default,
        change_column,
        modify_column,
        convert_to_charset,
        enable_keys,
        disable_keys,
        discard_tablespace,
        import_tablespace,
        // Specific DROP forms come before the generic column drop so a keyword
        // like INDEX or PRIMARY is not read as a column name.
        drop_index,
        drop_primary_key,
        drop_foreign_key,
        drop_column,
        force,
        change_lock,
        order_by,
        rename_index,
        rename,
        with_validation,
        without_validation,
        add_period_for_system_time,
    ];
    for rule in rules {
        let save = s.pos();
        if let Some(v) = rule(s) {
            return Some(wrap(v));
        }
        s.set(save);
    }
    None
}

/// Optional `COLUMN __`. The caller has already consumed the leading
/// whitespace. Used by ALTER, CHANGE, MODIFY, and DROP column rules.
fn opt_column(s: &mut Stream) {
    let save = s.pos();
    if s.eat_keyword("COLUMN").is_some() && s.ws1() {
    } else {
        s.set(save);
    }
}

/// Optional `__ COLUMN`. The caller has not consumed the leading whitespace and
/// consumes its own `__` after. Used by ADD column rules.
fn opt_ws_column(s: &mut Stream) {
    let save = s.pos();
    if s.ws1() && s.eat_keyword("COLUMN").is_some() {
    } else {
        s.set(save);
    }
}

/// Optional `__ ident`.
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

/// Optional `__ P_INDEX_TYPE`.
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

/// Optional `CONSTRAINT [ident]? __` prefix followed by `next`.
///
/// Returns Some(name-or-empty) when a constraint keyword was present, else None.
/// An empty string means present but unnamed, which the source treats as
/// undefined. The optional name is only kept when `next` follows it, mirroring
/// how the source backtracks when the name would swallow the type keyword.
fn opt_constraint(s: &mut Stream, next: &str) -> Option<String> {
    let save = s.pos();
    if s.eat_keyword("CONSTRAINT").is_none() {
        return None;
    }
    // Try with a name.
    let with_name = s.pos();
    if s.ws1() {
        if let Some(n) = s_identifier(s) {
            if s.ws1() && peek_keyword(s, next) {
                return Some(n);
            }
        }
    }
    // Retry without a name.
    s.set(with_name);
    if !s.ws1() {
        s.set(save);
        return None;
    }
    Some(String::new())
}

/// Whether the next token is the given keyword, without consuming it.
fn peek_keyword(s: &Stream, name: &str) -> bool {
    matches!(
        s.peek(),
        Some(crate::lexer::Token { kind: crate::lexer::TokenKind::Keyword(k), .. }) if k == name
    )
}

/// Optional trailing position: FIRST or AFTER ident.
fn opt_position(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.ws1() {
        if s.eat_keyword("FIRST").is_some() {
            return Some(json!({ "after": Value::Null }));
        }
        let asave = s.pos();
        if s.eat_keyword("AFTER").is_some() && s.ws1() {
            if let Some(id) = s_identifier(s) {
                return Some(json!({ "after": id }));
            }
        }
        s.set(asave);
    }
    s.set(save);
    None
}

/// Optional `__ P_COLUMN_REFERENCE`.
fn opt_reference(s: &mut Stream) -> Option<Value> {
    let save = s.pos();
    if s.ws1() {
        if let Some(r) = p_column_reference(s) {
            return Some(r);
        }
    }
    s.set(save);
    None
}

fn add_column(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ADD").is_none() {
        return None;
    }
    opt_ws_column(s);
    if !s.ws1() {
        return None;
    }
    let name = s_identifier(s)?;
    if !s.ws1() {
        return None;
    }
    let datatype = require_datatype(s)?;
    let column_definition = column_definitions(s);
    let reference = opt_reference(s);
    let position = opt_position(s);

    let mut def = Map::new();
    def.insert("action".into(), Value::String("addColumn".into()));
    def.insert("name".into(), Value::String(name));
    def.insert("datatype".into(), datatype);
    def.insert("columnDefinition".into(), Value::Array(column_definition));
    insert_position(&mut def, position);
    if let Some(r) = reference {
        def.insert("reference".into(), r);
    }
    Some(Value::Object(def))
}

fn add_columns(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ADD").is_none() {
        return None;
    }
    opt_ws_column(s);
    s.ws0();
    if s.eat(&TokenKind::LParens).is_none() {
        return None;
    }
    s.ws0();
    // First column.
    let first = add_columns_item(s)?;
    let mut columns = vec![first];
    loop {
        let sp = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Comma).is_none() {
            s.set(sp);
            break;
        }
        s.ws0();
        match add_columns_item(s) {
            Some(c) => columns.push(c),
            None => {
                s.set(sp);
                break;
            }
        }
    }
    s.ws0();
    if s.eat(&TokenKind::RParens).is_none() {
        return None;
    }
    Some(json!({
        "action": "addColumns",
        "columns": columns
    }))
}

fn add_columns_item(s: &mut Stream) -> Option<Value> {
    let name = s_identifier(s)?;
    if !s.ws1() {
        return None;
    }
    let datatype = require_datatype(s)?;
    let column_definition = column_definitions(s);
    let reference = opt_reference(s);
    let mut obj = Map::new();
    obj.insert("name".into(), Value::String(name));
    obj.insert("datatype".into(), datatype);
    obj.insert("columnDefinition".into(), Value::Array(column_definition));
    if let Some(r) = reference {
        obj.insert("reference".into(), r);
    }
    Some(Value::Object(obj))
}

fn add_index(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ADD").is_none() || !s.ws1() {
        return None;
    }
    if one_of_keywords(s, &["INDEX", "KEY"]).is_none() {
        return None;
    }
    let name = opt_ws_ident(s);
    let index = opt_index_type(s);
    let columns = index_column_list(s)?;
    let options = index_options(s);

    let mut def = Map::new();
    def.insert("action".into(), Value::String("addIndex".into()));
    insert_name_or_null(&mut def, name);
    insert_index_or_null(&mut def, index);
    def.insert("columns".into(), Value::Array(columns));
    def.insert("options".into(), Value::Array(options));
    Some(Value::Object(def))
}

fn add_primary_key(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ADD").is_none() || !s.ws1() {
        return None;
    }
    let constraint = opt_constraint(s, "PRIMARY");
    if s.eat_keyword("PRIMARY").is_none() || !s.ws1() || s.eat_keyword("KEY").is_none() {
        return None;
    }
    let index = opt_index_type(s);
    let columns = index_column_list(s)?;
    let options = index_options(s);

    let mut def = Map::new();
    def.insert("action".into(), Value::String("addPrimaryKey".into()));
    insert_constraint_name(&mut def, constraint);
    insert_index_or_null(&mut def, index);
    def.insert("columns".into(), Value::Array(columns));
    def.insert("options".into(), Value::Array(options));
    Some(Value::Object(def))
}

fn add_unique_key(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ADD").is_none() || !s.ws1() {
        return None;
    }
    let constraint = opt_constraint(s, "UNIQUE");
    if s.eat_keyword("UNIQUE").is_none() {
        return None;
    }
    // Optional __ INDEX | __ KEY.
    let sp = s.pos();
    if s.ws1() && one_of_keywords(s, &["INDEX", "KEY"]).is_some() {
    } else {
        s.set(sp);
    }
    // The index identifier is parsed but not used for the name here. The name
    // comes from the constraint prefix. The source applies the index/key
    // workaround to this identifier, which has no observable effect since it is
    // not read afterward.
    let _index_ident = opt_ws_ident(s);
    let index = opt_index_type(s);
    let columns = index_column_list(s)?;
    let options = index_options(s);

    let mut def = Map::new();
    def.insert("action".into(), Value::String("addUniqueKey".into()));
    insert_constraint_name(&mut def, constraint);
    insert_index_or_null(&mut def, index);
    def.insert("columns".into(), Value::Array(columns));
    def.insert("options".into(), Value::Array(options));
    Some(Value::Object(def))
}

fn add_fulltext(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ADD").is_none() || !s.ws1() || s.eat_keyword("FULLTEXT").is_none() {
        return None;
    }
    let sp = s.pos();
    if s.ws1() && one_of_keywords(s, &["INDEX", "KEY"]).is_some() {
    } else {
        s.set(sp);
    }
    let mut name = opt_ws_ident(s);
    let columns = index_column_list(s)?;
    let options = index_options(s);

    if let Some(n) = &name {
        let lower = n.to_lowercase();
        if lower == "index" || lower == "key" {
            name = None;
        }
    }

    let mut def = Map::new();
    def.insert("action".into(), Value::String("addFulltextIndex".into()));
    match name {
        Some(n) => {
            def.insert("name".into(), Value::String(n));
        }
        None => {
            def.insert("name".into(), Value::Null);
        }
    }
    def.insert("columns".into(), Value::Array(columns));
    def.insert("options".into(), Value::Array(options));
    Some(Value::Object(def))
}

fn add_spatial(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ADD").is_none() || !s.ws1() || s.eat_keyword("SPATIAL").is_none() {
        return None;
    }
    let sp = s.pos();
    if s.ws1() && one_of_keywords(s, &["INDEX", "KEY"]).is_some() {
    } else {
        s.set(sp);
    }
    let mut name = opt_ws_ident(s);
    let columns = index_column_list(s)?;
    let options = index_options(s);

    if let Some(n) = &name {
        let lower = n.to_lowercase();
        if lower == "index" || lower == "key" {
            name = None;
        }
    }

    let mut def = Map::new();
    def.insert("action".into(), Value::String("addSpatialIndex".into()));
    match name {
        Some(n) => {
            def.insert("name".into(), Value::String(n));
        }
        None => {
            def.insert("name".into(), Value::Null);
        }
    }
    def.insert("columns".into(), Value::Array(columns));
    def.insert("options".into(), Value::Array(options));
    Some(Value::Object(def))
}

fn add_foreign_key(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ADD").is_none() || !s.ws1() {
        return None;
    }
    let constraint = opt_constraint(s, "FOREIGN");
    if s.eat_keyword("FOREIGN").is_none() || !s.ws1() || s.eat_keyword("KEY").is_none() {
        return None;
    }
    let _index_name = opt_ws_ident(s);
    let columns = index_column_list(s)?;
    s.ws0();
    let reference = p_column_reference(s)?;

    let mut def = Map::new();
    def.insert("action".into(), Value::String("addForeignKey".into()));
    insert_constraint_name(&mut def, constraint);
    def.insert("columns".into(), Value::Array(columns));
    def.insert("reference".into(), reference);
    Some(Value::Object(def))
}

fn algorithm(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ALGORITHM").is_none() {
        return None;
    }
    if !ws_or_equals(s) {
        return None;
    }
    let v = one_of_keywords(s, &["DEFAULT", "INPLACE", "COPY"])?;
    Some(json!({ "action": "changeAlgorithm", "algorithm": v }))
}

fn set_default(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ALTER").is_none() || !s.ws1() {
        return None;
    }
    opt_column(s);
    let column = s_identifier(s)?;
    if !s.ws1() || s.eat_keyword("SET").is_none() || !s.ws1() || s.eat_keyword("DEFAULT").is_none()
    {
        return None;
    }
    if !s.ws1() {
        return None;
    }
    let value = o_default_value(s)?;
    Some(json!({
        "action": "setDefaultColumnValue",
        "column": column,
        "value": value
    }))
}

fn drop_default(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ALTER").is_none() || !s.ws1() {
        return None;
    }
    opt_column(s);
    let column = s_identifier(s)?;
    if !s.ws1() || s.eat_keyword("DROP").is_none() || !s.ws1() || s.eat_keyword("DEFAULT").is_none()
    {
        return None;
    }
    Some(json!({
        "action": "dropDefaultColumnValue",
        "column": column
    }))
}

fn change_column(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("CHANGE").is_none() || !s.ws1() {
        return None;
    }
    opt_column(s);
    let column = s_identifier(s)?;
    if !s.ws1() {
        return None;
    }
    let new_name = s_identifier(s)?;
    if !s.ws1() {
        return None;
    }
    let datatype = require_datatype(s)?;
    let column_definition = column_definitions(s);
    let reference = opt_reference(s);
    let position = opt_position(s);

    let mut def = Map::new();
    def.insert("action".into(), Value::String("changeColumn".into()));
    def.insert("column".into(), Value::String(column));
    def.insert("newName".into(), Value::String(new_name));
    def.insert("datatype".into(), datatype);
    def.insert("columnDefinition".into(), Value::Array(column_definition));
    insert_position(&mut def, position);
    if let Some(r) = reference {
        def.insert("reference".into(), r);
    }
    Some(Value::Object(def))
}

fn modify_column(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("MODIFY").is_none() || !s.ws1() {
        return None;
    }
    opt_column(s);
    let column = s_identifier(s)?;
    if !s.ws1() {
        return None;
    }
    let datatype = require_datatype(s)?;
    let column_definition = column_definitions(s);
    let reference = opt_reference(s);
    let position = opt_position(s);

    let mut def = Map::new();
    def.insert("action".into(), Value::String("changeColumn".into()));
    def.insert("column".into(), Value::String(column));
    // newName is undefined for MODIFY. JSON drops undefined, so leave it out.
    def.insert("datatype".into(), datatype);
    def.insert("columnDefinition".into(), Value::Array(column_definition));
    insert_position(&mut def, position);
    if let Some(r) = reference {
        def.insert("reference".into(), r);
    }
    Some(Value::Object(def))
}

fn convert_to_charset(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("CONVERT").is_none() || !s.ws1() || s.eat_keyword("TO").is_none() || !s.ws1() {
        return None;
    }
    // (CHARACTER SET | CHARSET).
    let matched = {
        let save = s.pos();
        if s.eat_keyword("CHARACTER").is_some() && s.ws1() && s.eat_keyword("SET").is_some() {
            true
        } else {
            s.set(save);
            s.eat_keyword("CHARSET").is_some()
        }
    };
    if !matched || !s.ws1() {
        return None;
    }
    let charset = super::helpers::o_string_or_ident(s)?;
    // Optional COLLATE collation.
    let mut collate: Option<String> = None;
    let sp = s.pos();
    if s.ws1() && s.eat_keyword("COLLATE").is_some() && s.ws1() {
        if let Some(c) = super::helpers::o_string_or_ident(s) {
            collate = Some(c);
        } else {
            s.set(sp);
        }
    } else {
        s.set(sp);
    }
    let mut def = Map::new();
    def.insert("action".into(), Value::String("convertToCharacterSet".into()));
    def.insert("charset".into(), Value::String(charset));
    // collate is a failed optional (null) when absent.
    def.insert(
        "collate".into(),
        collate.map(Value::String).unwrap_or(Value::Null),
    );
    Some(Value::Object(def))
}

fn enable_keys(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ENABLE").is_some() && s.ws1() && s.eat_keyword("KEYS").is_some() {
        return Some(json!({ "action": "enableKeys" }));
    }
    None
}

fn disable_keys(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("DISABLE").is_some() && s.ws1() && s.eat_keyword("KEYS").is_some() {
        return Some(json!({ "action": "disableKeys" }));
    }
    None
}

fn discard_tablespace(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("DISCARD").is_some() && s.ws1() && s.eat_keyword("TABLESPACE").is_some() {
        return Some(json!({ "action": "discardTablespace" }));
    }
    None
}

fn import_tablespace(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("IMPORT").is_some() && s.ws1() && s.eat_keyword("TABLESPACE").is_some() {
        return Some(json!({ "action": "importTablespace" }));
    }
    None
}

fn drop_column(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("DROP").is_none() || !s.ws1() {
        return None;
    }
    // COLUMN __.
    let sp = s.pos();
    if s.eat_keyword("COLUMN").is_some() && s.ws1() {
    } else {
        s.set(sp);
    }
    // IF EXISTS __.
    let sp = s.pos();
    if s.eat_keyword("IF").is_some()
        && s.ws1()
        && s.eat_keyword("EXISTS").is_some()
        && s.ws1()
    {
    } else {
        s.set(sp);
    }
    let column = s_identifier(s)?;
    Some(json!({ "action": "dropColumn", "column": column }))
}

fn drop_index(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("DROP").is_none() || !s.ws1() {
        return None;
    }
    if one_of_keywords(s, &["INDEX", "KEY"]).is_none() || !s.ws1() {
        return None;
    }
    let index = s_identifier(s)?;
    Some(json!({ "action": "dropIndex", "index": index }))
}

fn drop_primary_key(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("DROP").is_some()
        && s.ws1()
        && s.eat_keyword("PRIMARY").is_some()
        && s.ws1()
        && s.eat_keyword("KEY").is_some()
    {
        return Some(json!({ "action": "dropPrimaryKey" }));
    }
    None
}

fn drop_foreign_key(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("DROP").is_none() || !s.ws1() {
        return None;
    }
    if s.eat_keyword("FOREIGN").is_none() || !s.ws1() || s.eat_keyword("KEY").is_none() || !s.ws1() {
        return None;
    }
    let key = s_identifier(s)?;
    Some(json!({ "action": "dropForeignKey", "key": key }))
}

fn force(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("FORCE").is_some() {
        return Some(json!({ "action": "force" }));
    }
    None
}

fn change_lock(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("LOCK").is_none() {
        return None;
    }
    if !ws_or_equals(s) {
        return None;
    }
    let v = one_of_keywords(s, &["DEFAULT", "NONE", "SHARED", "EXCLUSIVE"])?;
    Some(json!({ "action": "changeLock", "lock": v }))
}

fn order_by(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ORDER").is_none() || !s.ws1() || s.eat_keyword("BY").is_none() || !s.ws1() {
        return None;
    }
    let first = s_identifier(s)?;
    let mut columns = vec![Value::String(first)];
    loop {
        let sp = s.pos();
        s.ws0();
        if s.eat(&TokenKind::Comma).is_none() {
            s.set(sp);
            break;
        }
        s.ws0();
        match s_identifier(s) {
            Some(c) => columns.push(Value::String(c)),
            None => {
                s.set(sp);
                break;
            }
        }
    }
    Some(json!({ "action": "orderBy", "columns": columns }))
}

fn rename_index(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("RENAME").is_none() || !s.ws1() {
        return None;
    }
    if one_of_keywords(s, &["INDEX", "KEY"]).is_none() || !s.ws1() {
        return None;
    }
    let index = s_identifier(s)?;
    if !s.ws1() || s.eat_keyword("TO").is_none() || !s.ws1() {
        return None;
    }
    let new_name = s_identifier(s)?;
    Some(json!({
        "action": "renameIndex",
        "index": index,
        "newName": new_name
    }))
}

fn rename(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("RENAME").is_none() || !s.ws1() {
        return None;
    }
    // TO __ | AS __.
    let sp = s.pos();
    if (s.eat_keyword("TO").is_some() || s.eat_keyword("AS").is_some()) && s.ws1() {
    } else {
        s.set(sp);
    }
    let new_name = s_identifier(s)?;
    Some(json!({ "action": "rename", "newName": new_name }))
}

fn with_validation(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("WITH").is_some() && s.ws1() && s.eat_keyword("VALIDATION").is_some() {
        return Some(json!({ "action": "withValidation" }));
    }
    None
}

fn without_validation(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("WITHOUT").is_some() && s.ws1() && s.eat_keyword("VALIDATION").is_some() {
        return Some(json!({ "action": "withoutValidation" }));
    }
    None
}

fn add_period_for_system_time(s: &mut Stream) -> Option<Value> {
    if s.eat_keyword("ADD").is_none() || !s.ws1() || s.eat_keyword("PERIOD").is_none() || !s.ws1() {
        return None;
    }
    if s.eat_keyword("FOR").is_none() || !s.ws1() || s.eat_keyword("SYSTEM_TIME").is_none() {
        return None;
    }
    s.ws0();
    if s.eat(&TokenKind::LParens).is_none() {
        return None;
    }
    s.ws0();
    let start = s_identifier(s)?;
    s.ws0();
    if s.eat(&TokenKind::Comma).is_none() {
        return None;
    }
    s.ws0();
    let end = s_identifier(s)?;
    s.ws0();
    if s.eat(&TokenKind::RParens).is_none() {
        return None;
    }
    Some(json!({
        "action": "addPeriodForSystemTime",
        "startColumnName": start,
        "endColumnName": end
    }))
}

/// Insert a constraint name into a spec. The source stores `name: d[2]` where
/// the constraint optional yields null when absent and an empty name when the
/// prefix is present but unnamed. Both serialize as null.
fn insert_constraint_name(def: &mut Map<String, Value>, constraint: Option<String>) {
    let value = match constraint {
        Some(name) if !name.is_empty() => Value::String(name),
        _ => Value::Null,
    };
    def.insert("name".into(), value);
}

/// Insert an optional index type, storing null when absent.
fn insert_index_or_null(def: &mut Map<String, Value>, index: Option<Value>) {
    def.insert("index".into(), index.unwrap_or(Value::Null));
}

/// Insert an optional name, storing null when absent.
fn insert_name_or_null(def: &mut Map<String, Value>, name: Option<String>) {
    def.insert("name".into(), name.map(Value::String).unwrap_or(Value::Null));
}

/// Insert an optional position, storing null when absent.
fn insert_position(def: &mut Map<String, Value>, position: Option<Value>) {
    def.insert("position".into(), position.unwrap_or(Value::Null));
}
