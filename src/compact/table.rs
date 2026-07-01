//! Table model and its mutations.
//!
//! Cross-table effects (renaming a table updates foreign keys in other tables)
//! are driven by [`Database`](super::database::Database), which owns all tables.
//! Methods here that need sibling tables take them as parameters.

use serde_json::{Map, Value};

use super::column::Column;
use super::keys::{ForeignKey, KeyIndex, PrimaryKey};
use super::table_options::TableOptions;

/// Which index collection an index lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexKind {
    Unique,
    Index,
    Fulltext,
    Spatial,
}

/// A position for inserting or moving a column.
#[derive(Debug, Clone)]
pub enum Position {
    /// Insert at the front.
    First,
    /// Insert after the named column.
    After(String),
}

/// A table as parsed from DDL.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub options: Option<TableOptions>,
    pub fulltext_indexes: Vec<KeyIndex>,
    pub spatial_indexes: Vec<KeyIndex>,
    pub foreign_keys: Vec<ForeignKey>,
    pub unique_keys: Vec<KeyIndex>,
    pub indexes: Vec<KeyIndex>,
    pub primary_key: Option<PrimaryKey>,
}

impl Table {
    /// Access the columns.
    pub fn columns_ref(&self) -> &[Column] {
        &self.columns
    }

    /// Get a column by name.
    pub fn get_column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Get a mutable column by name.
    pub fn get_column_mut(&mut self, name: &str) -> Option<&mut Column> {
        self.columns.iter_mut().find(|c| c.name == name)
    }

    /// Get a foreign key by name.
    pub fn get_foreign_key_index(&self, name: &str) -> Option<usize> {
        self.foreign_keys
            .iter()
            .position(|k| k.name.as_deref() == Some(name))
    }

    /// Build from a `P_CREATE_TABLE_COMMON` def.
    pub fn from_common_def(json: &Value) -> Table {
        let def = &json["def"];
        let mut table = Table {
            name: def["table"].as_str().unwrap_or("").to_string(),
            ..Default::default()
        };
        if let Some(opts) = def.get("tableOptions") {
            if !opts.is_null() {
                table.options = Some(TableOptions::from_def(opts));
            }
        }
        let create_defs = def["columnsDef"]["def"].as_array().cloned().unwrap_or_default();
        for cd in &create_defs {
            let inner = &cd["def"];
            if defined(inner, "column") {
                let column = Column::from_def(cd);
                table.add_column(column, None);
            } else if defined(inner, "fulltextIndex") {
                table.push_fulltext_index(KeyIndex::from_def(cd, "fulltextIndex"));
            } else if defined(inner, "spatialIndex") {
                table.push_spatial_index(KeyIndex::from_def(cd, "spatialIndex"));
            } else if defined(inner, "foreignKey") {
                table.push_foreign_key(ForeignKey::from_def(cd));
            } else if defined(inner, "uniqueKey") {
                table.push_unique_key(KeyIndex::from_def(cd, "uniqueKey"));
            } else if defined(inner, "primaryKey") {
                table.set_primary_key(PrimaryKey::from_def(cd));
            } else if defined(inner, "index") {
                table.push_index(KeyIndex::from_def(cd, "index"));
            }
        }
        table
    }

    /// Render to JSON in the source key order.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("name".into(), Value::String(self.name.clone()));
        m.insert(
            "columns".into(),
            Value::Array(self.columns.iter().map(Column::to_json).collect()),
        );
        if let Some(pk) = &self.primary_key {
            m.insert("primaryKey".into(), pk.to_json());
        }
        if !self.foreign_keys.is_empty() {
            m.insert(
                "foreignKeys".into(),
                Value::Array(self.foreign_keys.iter().map(ForeignKey::to_json).collect()),
            );
        }
        if !self.unique_keys.is_empty() {
            m.insert(
                "uniqueKeys".into(),
                Value::Array(self.unique_keys.iter().map(KeyIndex::to_json_unique).collect()),
            );
        }
        if !self.indexes.is_empty() {
            m.insert(
                "indexes".into(),
                Value::Array(self.indexes.iter().map(KeyIndex::to_json_index).collect()),
            );
        }
        if !self.spatial_indexes.is_empty() {
            m.insert(
                "spatialIndexes".into(),
                Value::Array(
                    self.spatial_indexes
                        .iter()
                        .map(KeyIndex::to_json_ft_spatial)
                        .collect(),
                ),
            );
        }
        if !self.fulltext_indexes.is_empty() {
            m.insert(
                "fulltextIndexes".into(),
                Value::Array(
                    self.fulltext_indexes
                        .iter()
                        .map(KeyIndex::to_json_ft_spatial)
                        .collect(),
                ),
            );
        }
        if let Some(o) = &self.options {
            m.insert("options".into(), o.to_json());
        }
        Value::Object(m)
    }

    /// Deep clone.
    pub fn clone_model(&self) -> Table {
        Table {
            name: self.name.clone(),
            columns: self.columns.iter().map(Column::clone_model).collect(),
            options: self.options.as_ref().map(TableOptions::clone_model),
            primary_key: self.primary_key.as_ref().map(PrimaryKey::clone_model),
            unique_keys: self.unique_keys.iter().map(KeyIndex::clone_model).collect(),
            foreign_keys: self.foreign_keys.iter().map(ForeignKey::clone_model).collect(),
            fulltext_indexes: self
                .fulltext_indexes
                .iter()
                .map(KeyIndex::clone_model)
                .collect(),
            spatial_indexes: self
                .spatial_indexes
                .iter()
                .map(KeyIndex::clone_model)
                .collect(),
            indexes: self.indexes.iter().map(KeyIndex::clone_model).collect(),
        }
    }

    /// Get the index array holding an index of the given name, searching in the
    /// order unique, index, fulltext, spatial.
    pub fn get_index_kind_by_name(&self, name: &str) -> Option<IndexKind> {
        if self.unique_keys.iter().any(|k| k.name.as_deref() == Some(name)) {
            return Some(IndexKind::Unique);
        }
        if self.indexes.iter().any(|k| k.name.as_deref() == Some(name)) {
            return Some(IndexKind::Index);
        }
        if self
            .fulltext_indexes
            .iter()
            .any(|k| k.name.as_deref() == Some(name))
        {
            return Some(IndexKind::Fulltext);
        }
        if self
            .spatial_indexes
            .iter()
            .any(|k| k.name.as_deref() == Some(name))
        {
            return Some(IndexKind::Spatial);
        }
        None
    }

    /// Whether a named index exists in any of the four collections.
    fn has_named_index(&self, name: &str) -> bool {
        self.get_index_kind_by_name(name).is_some()
    }

    /// Add a column at an optional position, then extract its keys.
    pub fn add_column(&mut self, column: Column, position: Option<Position>) {
        if self.get_column(&column.name).is_some() {
            return;
        }
        if column
            .options
            .as_ref()
            .map(|o| o.is_autoincrement())
            .unwrap_or(false)
            && self
                .columns
                .iter()
                .any(|c| c.options.as_ref().map(|o| o.is_autoincrement()).unwrap_or(false))
        {
            return;
        }
        if self.primary_key.is_some()
            && column.options.as_ref().map(|o| o.is_primary()).unwrap_or(false)
        {
            return;
        }

        match &position {
            None => self.columns.push(column.clone()),
            Some(Position::First) => self.columns.insert(0, column.clone()),
            Some(Position::After(after)) => {
                match self.columns.iter().position(|c| c.name == *after) {
                    Some(pos) => self.columns.insert(pos + 1, column.clone()),
                    None => return,
                }
            }
        }

        let name = column.name.clone();
        self.extract_column_keys(&name);
    }

    /// Extract primary, foreign, and unique keys out of a column's options.
    pub fn extract_column_keys(&mut self, column_name: &str) {
        let (pk, fk, uk) = {
            let col = match self.get_column_mut(column_name) {
                Some(c) => c,
                None => return,
            };
            (
                col.extract_primary_key(),
                col.extract_foreign_key(),
                col.extract_unique_key(),
            )
        };
        if let Some(pk) = pk {
            self.set_primary_key(pk);
        }
        if let Some(fk) = fk {
            self.push_foreign_key(fk);
        }
        if let Some(uk) = uk {
            self.push_unique_key(uk);
        }
    }

    /// Move a column to a position. Returns whether it succeeded.
    pub fn move_column(&mut self, column_name: &str, position: &Position) -> bool {
        let cur = match self.columns.iter().position(|c| c.name == column_name) {
            Some(p) => p,
            None => return false,
        };
        if let Position::After(after) = position {
            if !self.columns.iter().any(|c| c.name == *after) {
                return false;
            }
        }
        let col = self.columns.remove(cur);
        match position {
            Position::After(after) => {
                let pos = self.columns.iter().position(|c| c.name == *after).unwrap();
                self.columns.insert(pos + 1, col);
            }
            Position::First => self.columns.insert(0, col),
        }
        true
    }

    /// Current position of a column, for change-column without an explicit one.
    pub fn column_position(&self, column_name: &str) -> Position {
        let idx = self.columns.iter().position(|c| c.name == column_name);
        match idx {
            Some(0) | None => Position::First,
            Some(i) => Position::After(self.columns[i - 1].name.clone()),
        }
    }

    /// Set the primary key, applying nullable side effects.
    pub fn set_primary_key(&mut self, pk: PrimaryKey) {
        if self.primary_key.is_some() {
            return;
        }
        if !pk.has_all_columns(self) {
            return;
        }
        let names: Vec<String> = pk
            .columns
            .as_ref()
            .map(|cols| cols.iter().filter_map(|c| c.column.clone()).collect())
            .unwrap_or_default();
        for name in names {
            if let Some(col) = self.get_column_mut(&name) {
                if let Some(o) = &mut col.options {
                    o.set("nullable", Value::Bool(false));
                }
            }
        }
        self.primary_key = Some(pk);
    }

    /// Drop the primary key unless a key column is autoincrement.
    pub fn drop_primary_key(&mut self) {
        let pk = match &self.primary_key {
            Some(pk) => pk,
            None => return,
        };
        let has_autoincrement = pk
            .columns
            .as_ref()
            .map(|cols| {
                cols.iter().any(|ic| {
                    ic.column
                        .as_ref()
                        .and_then(|n| self.get_column(n))
                        .and_then(|c| c.options.as_ref())
                        .map(|o| o.is_autoincrement())
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if has_autoincrement {
            return;
        }
        self.primary_key = None;
    }

    /// Push a fulltext index if valid.
    pub fn push_fulltext_index(&mut self, index: KeyIndex) {
        if let Some(name) = &index.name {
            if self.has_named_index(name) {
                return;
            }
        }
        if !index.has_all_columns(self) {
            return;
        }
        self.fulltext_indexes.push(index);
    }

    /// Push a spatial index if valid.
    pub fn push_spatial_index(&mut self, index: KeyIndex) {
        if let Some(name) = &index.name {
            if self.has_named_index(name) {
                return;
            }
        }
        if !index.has_all_columns(self) {
            return;
        }
        self.spatial_indexes.push(index);
    }

    /// Push a unique key if valid, filling index sizes.
    pub fn push_unique_key(&mut self, mut key: KeyIndex) {
        if let Some(name) = &key.name {
            if self.has_named_index(name) {
                return;
            }
        }
        if !key.has_all_columns(self) {
            return;
        }
        key.set_index_size_from_table(self);
        self.unique_keys.push(key);
    }

    /// Push an index if valid, filling index sizes.
    pub fn push_index(&mut self, mut index: KeyIndex) {
        if let Some(name) = &index.name {
            if self.has_named_index(name) {
                return;
            }
        }
        if !index.has_all_columns(self) {
            return;
        }
        index.set_index_size_from_table(self);
        self.indexes.push(index);
    }

    /// Push a foreign key. The referenced-table check is disabled to match the
    /// source. Index sizes are filled from this table.
    pub fn push_foreign_key(&mut self, mut key: ForeignKey) {
        if let Some(name) = &key.name {
            if self.has_named_index(name) {
                return;
            }
        }
        key.set_index_size_from_table(self);
        self.foreign_keys.push(key);
    }

    /// Drop an index instance from whichever collection holds it.
    pub fn drop_index_by_kind(&mut self, kind: IndexKind, name: &str) {
        let list = match kind {
            IndexKind::Unique => &mut self.unique_keys,
            IndexKind::Index => &mut self.indexes,
            IndexKind::Fulltext => &mut self.fulltext_indexes,
            IndexKind::Spatial => &mut self.spatial_indexes,
        };
        if let Some(pos) = list.iter().position(|k| k.name.as_deref() == Some(name)) {
            list.remove(pos);
        }
    }
}

/// Whether a create-definition inner object defines the given key.
fn defined(inner: &Value, key: &str) -> bool {
    matches!(inner.get(key), Some(v) if !v.is_null())
}
