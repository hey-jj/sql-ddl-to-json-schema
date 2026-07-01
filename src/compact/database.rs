//! Database replay: applies each DDL statement to the accumulated tables.

use serde_json::Value;

use super::column::{Column, ColumnObject};
use super::keys::{ForeignKey, KeyIndex, PrimaryKey};
use super::models::{ColumnOptions, Datatype};
use super::table::{IndexKind, Position, Table};
use super::table_options::TableOptions;
use super::util::is_defined;

/// The in-memory database that tables are built into.
pub struct Database {
    pub tables: Vec<Table>,
}

impl Database {
    /// Empty database.
    pub fn new() -> Database {
        Database { tables: Vec::new() }
    }

    /// Index of a table by name.
    fn table_index(&self, name: &str) -> Option<usize> {
        self.tables.iter().position(|t| t.name == name)
    }

    /// Push a table, ignoring a duplicate name.
    fn push_table(&mut self, table: Table) {
        if self.tables.iter().any(|t| t.name == table.name) {
            return;
        }
        self.tables.push(table);
    }

    /// Replay all statements.
    pub fn parse_dds_collection(&mut self, dds: &[Value]) {
        for d in dds {
            if d.is_null() {
                continue;
            }
            let json = &d["def"];
            match json.get("id").and_then(Value::as_str) {
                Some("P_CREATE_TABLE") => self.handle_create_table(json),
                Some("P_CREATE_INDEX") => self.handle_create_index(json),
                Some("P_ALTER_TABLE") => self.handle_alter_table(json),
                Some("P_RENAME_TABLE") => self.handle_rename_table(json),
                Some("P_DROP_TABLE") => self.handle_drop_table(json),
                Some("P_DROP_INDEX") => self.handle_drop_index(json),
                _ => {}
            }
        }
    }

    fn handle_create_table(&mut self, json: &Value) {
        let def = &json["def"];
        match def.get("id").and_then(Value::as_str) {
            Some("P_CREATE_TABLE_COMMON") => {
                let table = Table::from_common_def(def);
                self.push_table(table);
            }
            Some("P_CREATE_TABLE_LIKE") => {
                let like = def["def"]["like"].as_str().unwrap_or("");
                let name = def["def"]["table"].as_str().unwrap_or("").to_string();
                if let Some(src_idx) = self.table_index(like) {
                    let mut table = self.tables[src_idx].clone_model();
                    table.name = name;
                    table.foreign_keys = Vec::new();
                    self.push_table(table);
                }
            }
            _ => {}
        }
    }

    fn handle_create_index(&mut self, json: &Value) {
        let table_name = json["def"]["table"].as_str().unwrap_or("");
        let idx = match self.table_index(table_name) {
            Some(i) => i,
            None => return,
        };
        let type_str = json["def"]["type"].as_str().unwrap_or("").to_lowercase();
        let table = &mut self.tables[idx];
        if type_str.contains("unique") {
            table.push_unique_key(KeyIndex::from_def(json, "uniqueKey"));
        } else if type_str.contains("fulltext") {
            table.push_fulltext_index(KeyIndex::from_def(json, "fulltextIndex"));
        } else if type_str.contains("spatial") {
            table.push_spatial_index(KeyIndex::from_def(json, "spatialIndex"));
        } else {
            table.push_index(KeyIndex::from_def(json, "index"));
        }
    }

    fn handle_rename_table(&mut self, json: &Value) {
        let pairs = json["def"].as_array().cloned().unwrap_or_default();
        for pair in pairs {
            let from = pair["table"].as_str().unwrap_or("");
            let new_name = pair["newName"].as_str().unwrap_or("").to_string();
            if self.table_index(from).is_some() {
                self.rename_table(from, &new_name);
            }
        }
    }

    /// Rename a table, updating foreign keys in every table that references it.
    fn rename_table(&mut self, from: &str, new_name: &str) {
        for t in &mut self.tables {
            for fk in &mut t.foreign_keys {
                if fk.references_table(from) {
                    fk.update_referenced_table_name(new_name);
                }
            }
        }
        if let Some(idx) = self.table_index(from) {
            self.tables[idx].name = new_name.to_string();
        }
    }

    fn handle_drop_table(&mut self, json: &Value) {
        let names: Vec<String> = json["def"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        for name in names {
            let idx = match self.table_index(&name) {
                Some(i) => i,
                None => continue,
            };
            let has_reference = self
                .tables
                .iter()
                .any(|t| t.foreign_keys.iter().any(|k| k.references_table(&name)));
            if has_reference {
                continue;
            }
            self.tables.remove(idx);
        }
    }

    fn handle_drop_index(&mut self, json: &Value) {
        let table_name = json["def"]["table"].as_str().unwrap_or("");
        let index_name = json["def"]["index"].as_str().unwrap_or("").to_string();
        let idx = match self.table_index(table_name) {
            Some(i) => i,
            None => return,
        };
        let table = &mut self.tables[idx];
        if let Some(kind) = table.get_index_kind_by_name(&index_name) {
            table.drop_index_by_kind(kind, &index_name);
        }
    }

    fn handle_alter_table(&mut self, json: &Value) {
        let table_name = json["def"]["table"].as_str().unwrap_or("").to_string();
        if self.table_index(&table_name).is_none() {
            return;
        }
        let specs = json["def"]["specs"].as_array().cloned().unwrap_or_default();
        for spec in specs {
            let def = &spec["def"];
            if let Some(change) = def.get("spec").filter(|v| !v.is_null()) {
                let action = change["def"]["action"].as_str().unwrap_or("");
                let action_def = &change["def"];
                self.alter_action(&table_name, action, action_def);
            } else if let Some(table_options) = def.get("tableOptions").filter(|v| !v.is_null()) {
                let idx = self.table_index(&table_name).unwrap();
                let opts = TableOptions::from_def(table_options);
                let table = &mut self.tables[idx];
                if table.options.is_none() {
                    table.options = Some(TableOptions::default());
                }
                table.options.as_mut().unwrap().merge_with(&opts);
            }
        }
    }

    /// Dispatch one alter action by name.
    fn alter_action(&mut self, table_name: &str, action: &str, json: &Value) {
        match action {
            "addColumn" => self.alter_add_column(table_name, json),
            "addColumns" => self.alter_add_columns(table_name, json),
            "addIndex" => self.with_table(table_name, |t| {
                t.push_index(KeyIndex::from_object(json));
            }),
            "addPrimaryKey" => self.with_table(table_name, |t| {
                t.set_primary_key(PrimaryKey::from_object(json));
            }),
            "addUniqueKey" => self.with_table(table_name, |t| {
                t.push_unique_key(KeyIndex::from_object(json));
            }),
            "addFulltextIndex" => self.with_table(table_name, |t| {
                t.push_fulltext_index(KeyIndex::from_object(json));
            }),
            // Preserve the source bug: spatial index add pushes to fulltext.
            "addSpatialIndex" => self.with_table(table_name, |t| {
                t.push_fulltext_index(KeyIndex::from_object(json));
            }),
            "addForeignKey" => self.with_table(table_name, |t| {
                t.push_foreign_key(ForeignKey::from_object(json));
            }),
            "setDefaultColumnValue" => self.alter_set_default(table_name, json),
            "dropDefaultColumnValue" => self.alter_drop_default(table_name, json),
            "changeColumn" => self.alter_change_column(table_name, json),
            "dropColumn" => self.alter_drop_column(table_name, json),
            "dropIndex" => self.alter_drop_index(table_name, json),
            "dropPrimaryKey" => self.with_table(table_name, |t| t.drop_primary_key()),
            "dropForeignKey" => self.alter_drop_foreign_key(table_name, json),
            "renameIndex" => self.alter_rename_index(table_name, json),
            "rename" => {
                let new_name = json["newName"].as_str().unwrap_or("").to_string();
                self.rename_table(table_name, &new_name);
            }
            _ => {}
        }
    }

    /// Run a closure with a mutable table by name.
    fn with_table<F: FnOnce(&mut Table)>(&mut self, name: &str, f: F) {
        if let Some(idx) = self.table_index(name) {
            f(&mut self.tables[idx]);
        }
    }

    fn alter_add_column(&mut self, table_name: &str, json: &Value) {
        let obj = ColumnObject {
            name: json["name"].as_str().unwrap_or("").to_string(),
            datatype: json["datatype"].clone(),
            reference: None, // reference is deleted before adding
            column_definition: json.get("columnDefinition").cloned(),
        };
        let column = Column::from_object(&obj);
        let position = read_position(json.get("position"));
        self.with_table(table_name, |t| t.add_column(column, position));
    }

    fn alter_add_columns(&mut self, table_name: &str, json: &Value) {
        let cols = json["columns"].as_array().cloned().unwrap_or_default();
        for c in cols {
            let obj = ColumnObject {
                name: c["name"].as_str().unwrap_or("").to_string(),
                datatype: c["datatype"].clone(),
                reference: None,
                column_definition: c.get("columnDefinition").cloned(),
            };
            let column = Column::from_object(&obj);
            self.with_table(table_name, |t| t.add_column(column, None));
        }
    }

    fn alter_set_default(&mut self, table_name: &str, json: &Value) {
        let column = json["column"].as_str().unwrap_or("").to_string();
        let value = json["value"].clone();
        self.with_table(table_name, |t| {
            if let Some(col) = t.get_column_mut(&column) {
                if let Some(o) = &mut col.options {
                    o.set("default", value);
                }
            }
        });
    }

    fn alter_drop_default(&mut self, table_name: &str, json: &Value) {
        let column = json["column"].as_str().unwrap_or("").to_string();
        self.with_table(table_name, |t| {
            if let Some(col) = t.get_column_mut(&column) {
                if let Some(o) = &mut col.options {
                    o.remove("default");
                }
            }
        });
    }

    fn alter_change_column(&mut self, table_name: &str, json: &Value) {
        let idx = match self.table_index(table_name) {
            Some(i) => i,
            None => return,
        };
        let column_name = json["column"].as_str().unwrap_or("").to_string();
        if self.tables[idx].get_column(&column_name).is_none() {
            return;
        }

        // Resolve position.
        let position = match read_position(json.get("position")) {
            Some(p) => {
                if let Position::After(after) = &p {
                    if self.tables[idx].get_column(after).is_none() {
                        return;
                    }
                }
                p
            }
            None => self.tables[idx].column_position(&column_name),
        };

        let datatype = Datatype::from_def(&json["datatype"]);
        let options = match json.get("columnDefinition") {
            Some(cd) if !cd.is_null() => {
                ColumnOptions::from_array(&cd.as_array().cloned().unwrap_or_default())
            }
            _ => return,
        };

        // Cancel if it would overwrite an existing primary key.
        if options.is_primary() && self.tables[idx].primary_key.is_some() {
            return;
        }
        // Cancel if another column is already autoincrement.
        if options.is_autoincrement()
            && self.tables[idx].columns.iter().any(|c| {
                c.name != column_name
                    && c.options
                        .as_ref()
                        .map(|o| o.is_autoincrement())
                        .unwrap_or(false)
            })
        {
            return;
        }
        // Drop a unique that would duplicate an existing single-column unique.
        let mut options = options;
        if options.is_unique()
            && self.tables[idx].unique_keys.iter().any(|uk| {
                uk.columns.len() == 1 && uk.columns[0].column.as_deref() == Some(&column_name)
            })
        {
            options.remove("unique");
        }

        // Move, rename, then set type and options.
        if self.tables[idx].move_column(&column_name, &position) {
            let new_name = json.get("newName").and_then(Value::as_str);
            if let Some(new_name) = new_name {
                if new_name != column_name {
                    self.rename_column(idx, &column_name, new_name);
                }
            }
            let effective_name = new_name
                .filter(|n| *n != column_name)
                .map(String::from)
                .unwrap_or(column_name.clone());
            if let Some(col) = self.tables[idx].get_column_mut(&effective_name) {
                col.datatype = datatype;
                col.options = Some(options);
            }
            self.tables[idx].extract_column_keys(&effective_name);
        }
    }

    /// Rename a column and its references across all tables.
    fn rename_column(&mut self, table_idx: usize, old: &str, new: &str) {
        let table_name = self.tables[table_idx].name.clone();
        // Update foreign keys in other tables that reference this table.
        for t in &mut self.tables {
            for fk in &mut t.foreign_keys {
                if fk.references_table(&table_name) {
                    fk.rename_column(old, new);
                }
            }
        }
        // Update index collections and primary key in this table.
        let table = &mut self.tables[table_idx];
        for i in &mut table.fulltext_indexes {
            i.rename_column(old, new);
        }
        for i in &mut table.spatial_indexes {
            i.rename_column(old, new);
        }
        for i in &mut table.indexes {
            i.rename_column(old, new);
        }
        for k in &mut table.unique_keys {
            k.rename_column(old, new);
        }
        if let Some(pk) = &mut table.primary_key {
            pk.rename_column(old, new);
        }
        if let Some(col) = table.get_column_mut(old) {
            col.name = new.to_string();
        }
    }

    fn alter_drop_column(&mut self, table_name: &str, json: &Value) {
        let idx = match self.table_index(table_name) {
            Some(i) => i,
            None => return,
        };
        let column_name = json["column"].as_str().unwrap_or("").to_string();
        if self.tables[idx].get_column(&column_name).is_none() {
            return;
        }
        self.drop_column(idx, &column_name);
    }

    /// Drop a column with all the source guards and index cleanup.
    fn drop_column(&mut self, table_idx: usize, column_name: &str) {
        let table_name = self.tables[table_idx].name.clone();
        // Guard: any foreign key referencing this table and column.
        let referenced = self.tables.iter().any(|t| {
            t.foreign_keys
                .iter()
                .any(|k| k.references_table_and_column(&table_name, column_name))
        });
        if referenced {
            return;
        }
        let table = &mut self.tables[table_idx];
        // Guard: do not drop the last column.
        if table.columns.len() == 1 {
            return;
        }
        let pos = match table.columns.iter().position(|c| c.name == column_name) {
            Some(p) => p,
            None => return,
        };
        table.columns.remove(pos);

        // Remove the column from indexes, dropping empty indexes.
        clean_index_list(&mut table.fulltext_indexes, column_name);
        clean_index_list(&mut table.spatial_indexes, column_name);
        clean_index_list(&mut table.indexes, column_name);
        clean_index_list(&mut table.unique_keys, column_name);

        // Foreign keys.
        let mut i = 0;
        while i < table.foreign_keys.len() {
            let removed = table.foreign_keys[i].drop_column(column_name);
            if removed && table.foreign_keys[i].columns.is_empty() {
                table.foreign_keys.remove(i);
            } else {
                i += 1;
            }
        }

        // Primary key.
        if let Some(pk) = &mut table.primary_key {
            let removed = pk.drop_column(column_name);
            let empty = pk.columns.as_ref().map(|c| c.is_empty()).unwrap_or(true);
            if removed && empty {
                table.primary_key = None;
            }
        }
    }

    fn alter_drop_index(&mut self, table_name: &str, json: &Value) {
        let index = json["index"].as_str().unwrap_or("").to_string();
        if index.to_lowercase() == "primary" {
            self.with_table(table_name, |t| t.drop_primary_key());
            return;
        }
        self.with_table(table_name, |t| {
            if let Some(kind) = t.get_index_kind_by_name(&index) {
                t.drop_index_by_kind(kind, &index);
            }
        });
    }

    fn alter_drop_foreign_key(&mut self, table_name: &str, json: &Value) {
        let key = json["key"].as_str().unwrap_or("").to_string();
        self.with_table(table_name, |t| {
            if let Some(pos) = t.get_foreign_key_index(&key) {
                t.foreign_keys.remove(pos);
            }
        });
    }

    fn alter_rename_index(&mut self, table_name: &str, json: &Value) {
        let index = json["index"].as_str().unwrap_or("").to_string();
        let new_name = json["newName"].as_str().unwrap_or("").to_string();
        self.with_table(table_name, |t| {
            if let Some(kind) = t.get_index_kind_by_name(&index) {
                let list = match kind {
                    IndexKind::Unique => &mut t.unique_keys,
                    IndexKind::Index => &mut t.indexes,
                    IndexKind::Fulltext => &mut t.fulltext_indexes,
                    IndexKind::Spatial => &mut t.spatial_indexes,
                };
                if let Some(k) = list.iter_mut().find(|k| k.name.as_deref() == Some(&index)) {
                    k.name = Some(new_name.clone());
                }
            }
        });
    }
}

impl Default for Database {
    fn default() -> Self {
        Database::new()
    }
}

/// Remove a column from an index list, dropping indexes that become empty.
fn clean_index_list(list: &mut Vec<KeyIndex>, column_name: &str) {
    let mut i = 0;
    while i < list.len() {
        let removed = list[i].drop_column(column_name);
        if removed && list[i].columns.is_empty() {
            list.remove(i);
        } else {
            i += 1;
        }
    }
}

/// Read a position object into a `Position`.
fn read_position(pos: Option<&Value>) -> Option<Position> {
    let pos = pos?;
    if !is_defined(Some(pos)) {
        return None;
    }
    let after = pos.get("after");
    match after {
        Some(Value::Null) => Some(Position::First),
        Some(Value::String(s)) => Some(Position::After(s.clone())),
        _ => None,
    }
}
