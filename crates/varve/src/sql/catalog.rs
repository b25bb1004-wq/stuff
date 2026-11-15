//! Persistent catalog management in the page-0 B+tree.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use crate::sql::ast::{ColType, CreateIndexStmt, CreateTableStmt, Stmt};
use crate::sql::codec::{decode_record, decode_rowid_key, encode_key, encode_record};
use crate::sql::lexer::parse_err;
use crate::sql::parser::parse_statement;
use crate::sql::Value;
use crate::{Error, Pager, Storage, Tree};

const MAX_KEY_BYTES: usize = 1024;

#[derive(Debug, Clone)]
pub(crate) struct ColumnDef {
    pub name: String,
    pub ty: ColType,
    pub is_ipk: bool,
    pub not_null: bool,
    pub unique: bool,
    pub primary_key: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TableDef {
    pub name: String,
    pub root: u64,
    pub columns: Vec<ColumnDef>,
    pub ipk_col: Option<usize>,
    pub has_rowid_col: bool,
    pub sql: String,
    pub max_rowid: Option<i64>,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexDef {
    pub name: String,
    pub table: String,
    pub root: u64,
    pub col_indices: Vec<usize>,
    pub unique: bool,
    pub auto: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Catalog {
    pub tables: BTreeMap<String, TableDef>,
    pub indexes: BTreeMap<String, IndexDef>,
    pub index_order: Vec<String>,
    pub next_seq: i64,
}

impl Catalog {
    pub fn load<S: Storage>(pager: &mut Pager<S>, catalog_root: u64) -> Result<Self, Error> {
        let tree = Tree::open(catalog_root);
        let mut raw_tables = Vec::new();
        let mut raw_indexes = Vec::new();
        let mut scan_err = None;

        let mut max_seq = 0i64;
        tree.scan(pager, None, Bound::Unbounded, &mut |k, v| {
            let res = (|| -> Result<(), Error> {
                let obj_name = std::str::from_utf8(k)
                    .map_err(|_| Error::Corrupt)?
                    .to_owned();
                let vals = decode_record(v)?;
                if vals.len() != 4 && vals.len() != 5 {
                    return Err(Error::Corrupt);
                }
                let (
                    Value::Text(kind),
                    Value::Text(tbl_name),
                    Value::Int(root_i64),
                    Value::Text(sql),
                ) = (&vals[0], &vals[1], &vals[2], &vals[3])
                else {
                    return Err(Error::Corrupt);
                };
                let seq = if vals.len() == 5 {
                    let Value::Int(s) = vals[4] else {
                        return Err(Error::Corrupt);
                    };
                    s
                } else {
                    *root_i64
                };
                max_seq = max_seq.max(seq);
                let root = u64::try_from(*root_i64).map_err(|_| Error::Corrupt)?;
                match kind.as_str() {
                    "table" => raw_tables.push((obj_name, root, sql.clone())),
                    "index" => {
                        raw_indexes.push((obj_name, tbl_name.clone(), root, sql.clone(), seq))
                    }
                    _ => return Err(Error::Corrupt),
                }
                Ok(())
            })();
            if let Err(e) = res {
                scan_err = Some(e);
                return false;
            }
            true
        })?;
        if let Some(e) = scan_err {
            return Err(e);
        }

        let mut cat = Catalog::default();
        for (_name, root, sql) in raw_tables {
            let Stmt::CreateTable(ct) = parse_statement(&sql).map_err(|_| Error::Corrupt)? else {
                return Err(Error::Corrupt);
            };
            let tdef = build_table_def(&ct, root, sql)?;
            cat.tables.insert(tdef.name.to_ascii_lowercase(), tdef);
        }

        let mut loaded_indexes = Vec::new();
        for (name, _tbl, root, sql, seq) in raw_indexes {
            let Stmt::CreateIndex(ci) = parse_statement(&sql).map_err(|_| Error::Corrupt)? else {
                return Err(Error::Corrupt);
            };
            let table = cat
                .tables
                .get(&ci.table.to_ascii_lowercase())
                .ok_or(Error::Corrupt)?;
            let mut col_indices = Vec::with_capacity(ci.columns.len());
            for (cname, _) in &ci.columns {
                let idx = table
                    .columns
                    .iter()
                    .position(|c| c.name.eq_ignore_ascii_case(cname))
                    .ok_or(Error::Corrupt)?;
                col_indices.push(idx);
            }
            let auto = name.starts_with("sqlite_autoindex_");
            let idef = IndexDef {
                name: ci.name.clone(),
                table: table.name.clone(),
                root,
                col_indices,
                unique: ci.unique,
                auto,
            };
            loaded_indexes.push((seq, idef));
        }
        loaded_indexes.sort_by_key(|(seq, i)| (u8::from(!i.auto), *seq));
        for (_, idef) in loaded_indexes {
            let key = idef.name.to_ascii_lowercase();
            cat.index_order.push(key.clone());
            cat.indexes.insert(key, idef);
        }
        cat.next_seq = max_seq.saturating_add(1).max(1);

        Ok(cat)
    }

    pub fn object_exists(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        self.tables.contains_key(&lower) || self.indexes.contains_key(&lower)
    }

    pub fn table(&self, name: &str) -> Result<&TableDef, Error> {
        self.tables
            .get(&name.to_ascii_lowercase())
            .ok_or(Error::NoSuchTable)
    }

    pub fn table_mut(&mut self, name: &str) -> Result<&mut TableDef, Error> {
        self.tables
            .get_mut(&name.to_ascii_lowercase())
            .ok_or(Error::NoSuchTable)
    }

    pub fn table_indexes(&self, table_name: &str) -> Vec<&IndexDef> {
        self.index_order
            .iter()
            .filter_map(|k| self.indexes.get(k))
            .filter(|i| i.table.eq_ignore_ascii_case(table_name))
            .collect()
    }

    pub fn alloc_seq(&mut self) -> i64 {
        let s = self.next_seq.max(1);
        self.next_seq = s.saturating_add(1);
        s
    }
}

fn build_table_def(ct: &CreateTableStmt, root: u64, sql: String) -> Result<TableDef, Error> {
    let mut seen = BTreeSet::new();
    let mut pk_count = 0usize;
    let mut ipk_col = None;
    let mut columns = Vec::with_capacity(ct.columns.len());
    for (idx, c) in ct.columns.iter().enumerate() {
        let lower = c.name.to_ascii_lowercase();
        if !seen.insert(lower) {
            return Err(Error::Exists);
        }
        if c.primary_key {
            pk_count += 1;
            if pk_count > 1 {
                return Err(parse_err(c.name_pos, "table can have only one PRIMARY KEY"));
            }
        }
        let is_ipk = c.is_integer_keyword && c.primary_key;
        if is_ipk {
            ipk_col = Some(idx);
        }
        columns.push(ColumnDef {
            name: c.name.clone(),
            ty: c.ty,
            is_ipk,
            not_null: c.not_null,
            unique: c.unique,
            primary_key: c.primary_key,
        });
    }
    let has_rowid_col = columns.iter().any(|c| c.name.eq_ignore_ascii_case("rowid"));
    Ok(TableDef {
        name: ct.name.clone(),
        root,
        columns,
        ipk_col,
        has_rowid_col,
        sql,
        max_rowid: None,
    })
}

pub(crate) fn exec_create_table<S: Storage>(
    pager: &mut Pager<S>,
    cat: &mut Catalog,
    catalog_root: u64,
    stmt: &CreateTableStmt,
    raw_sql: &str,
) -> Result<(), Error> {
    if cat.object_exists(&stmt.name) {
        if stmt.if_not_exists {
            return Ok(());
        }
        return Err(Error::Exists);
    }
    if stmt
        .name
        .to_ascii_lowercase()
        .starts_with("sqlite_autoindex_")
    {
        return Err(parse_err(stmt.name_pos, "reserved object name"));
    }
    let tdef_preview = build_table_def(stmt, 0, raw_sql.to_owned())?;

    let cat_tree = Tree::open(catalog_root);
    let table_tree = Tree::create(pager)?;
    let mut tdef = tdef_preview;
    tdef.root = table_tree.root();
    tdef.max_rowid = Some(0);

    let tbl_seq = cat.alloc_seq();
    let rec = encode_record(&[
        Value::Text("table".to_owned()),
        Value::Text(tdef.name.clone()),
        Value::Int(tdef.root as i64),
        Value::Text(tdef.sql.clone()),
        Value::Int(tbl_seq),
    ]);
    cat_tree.insert(pager, tdef.name.as_bytes(), &rec)?;

    let mut auto_num = 1usize;
    for (col_idx, col) in tdef.columns.iter().enumerate() {
        let needs_auto = !col.is_ipk && (col.unique || col.primary_key);
        if needs_auto {
            let idx_name = format!(
                "sqlite_autoindex_{}_{}",
                tdef.name.to_ascii_lowercase(),
                auto_num
            );
            auto_num += 1;
            let idx_tree = Tree::create(pager)?;
            let idx_sql = format!(
                "CREATE UNIQUE INDEX \"{}\" ON \"{}\" (\"{}\")",
                idx_name, tdef.name, col.name
            );
            let idx_seq = cat.alloc_seq();
            let idx_rec = encode_record(&[
                Value::Text("index".to_owned()),
                Value::Text(tdef.name.clone()),
                Value::Int(idx_tree.root() as i64),
                Value::Text(idx_sql.clone()),
                Value::Int(idx_seq),
            ]);
            cat_tree.insert(pager, idx_name.as_bytes(), &idx_rec)?;
            let key = idx_name.to_ascii_lowercase();
            cat.index_order.push(key.clone());
            cat.indexes.insert(
                key,
                IndexDef {
                    name: idx_name,
                    table: tdef.name.clone(),
                    root: idx_tree.root(),
                    col_indices: vec![col_idx],
                    unique: true,
                    auto: true,
                },
            );
        }
    }

    cat.tables.insert(tdef.name.to_ascii_lowercase(), tdef);
    Ok(())
}

pub(crate) fn exec_drop_table<S: Storage>(
    pager: &mut Pager<S>,
    cat: &mut Catalog,
    catalog_root: u64,
    name: &str,
    if_exists: bool,
) -> Result<(), Error> {
    let lower = name.to_ascii_lowercase();
    let Some(tdef) = cat.tables.get(&lower).cloned() else {
        if if_exists {
            return Ok(());
        }
        return Err(Error::NoSuchTable);
    };
    let cat_tree = Tree::open(catalog_root);

    let idx_keys: Vec<String> = cat
        .indexes
        .iter()
        .filter(|(_, i)| i.table.eq_ignore_ascii_case(&tdef.name))
        .map(|(k, _)| k.clone())
        .collect();
    for k in idx_keys {
        if let Some(idef) = cat.indexes.remove(&k) {
            cat.index_order.retain(|x| x != &k);
            Tree::open(idef.root).destroy(pager)?;
            cat_tree.remove(pager, idef.name.as_bytes())?;
        }
    }

    cat.tables.remove(&lower);
    Tree::open(tdef.root).destroy(pager)?;
    cat_tree.remove(pager, tdef.name.as_bytes())?;
    Ok(())
}

pub(crate) fn exec_create_index<S: Storage>(
    pager: &mut Pager<S>,
    cat: &mut Catalog,
    catalog_root: u64,
    stmt: &CreateIndexStmt,
    raw_sql: &str,
) -> Result<(), Error> {
    if cat.object_exists(&stmt.name) {
        if stmt.if_not_exists {
            return Ok(());
        }
        return Err(Error::Exists);
    }
    if stmt
        .name
        .to_ascii_lowercase()
        .starts_with("sqlite_autoindex_")
    {
        return Err(parse_err(stmt.name_pos, "reserved object name"));
    }
    let table = cat.table(&stmt.table)?.clone();
    let mut col_indices = Vec::with_capacity(stmt.columns.len());
    for (cname, _) in &stmt.columns {
        let idx = table
            .columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(cname))
            .ok_or(Error::NoSuchColumn)?;
        col_indices.push(idx);
    }

    // Plan and validate all index entries before allocating pages.
    let mut entries: Vec<Vec<u8>> = Vec::new();
    let mut unique_check: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut scan_err = None;
    Tree::open(table.root).scan(pager, None, Bound::Unbounded, &mut |k, v| {
        let res = (|| -> Result<(), Error> {
            let rowid = decode_rowid_key(k)?;
            let row = decode_record(v)?;
            let mut key_vals = Vec::with_capacity(col_indices.len() + 1);
            let mut has_null = false;
            for &ci in &col_indices {
                let val = row.get(ci).cloned().unwrap_or(Value::Null);
                if matches!(val, Value::Null) {
                    has_null = true;
                }
                key_vals.push(val);
            }
            if stmt.unique && !has_null {
                let prefix = encode_key(&key_vals);
                if !unique_check.insert(prefix) {
                    return Err(Error::Constraint(format!(
                        "UNIQUE constraint failed: {}",
                        stmt.name
                    )));
                }
            }
            key_vals.push(Value::Int(rowid));
            let full_key = encode_key(&key_vals);
            if full_key.len() > MAX_KEY_BYTES {
                return Err(Error::TooLarge);
            }
            entries.push(full_key);
            Ok(())
        })();
        if let Err(e) = res {
            scan_err = Some(e);
            return false;
        }
        true
    })?;
    if let Some(e) = scan_err {
        return Err(e);
    }

    let idx_tree = Tree::create(pager)?;
    for key in &entries {
        idx_tree.insert(pager, key, &[])?;
    }

    let cat_tree = Tree::open(catalog_root);
    let idx_seq = cat.alloc_seq();
    let rec = encode_record(&[
        Value::Text("index".to_owned()),
        Value::Text(table.name.clone()),
        Value::Int(idx_tree.root() as i64),
        Value::Text(raw_sql.to_owned()),
        Value::Int(idx_seq),
    ]);
    cat_tree.insert(pager, stmt.name.as_bytes(), &rec)?;

    let key = stmt.name.to_ascii_lowercase();
    cat.index_order.push(key.clone());
    cat.indexes.insert(
        key,
        IndexDef {
            name: stmt.name.clone(),
            table: table.name,
            root: idx_tree.root(),
            col_indices,
            unique: stmt.unique,
            auto: false,
        },
    );
    Ok(())
}

pub(crate) fn exec_drop_index<S: Storage>(
    pager: &mut Pager<S>,
    cat: &mut Catalog,
    catalog_root: u64,
    name: &str,
    if_exists: bool,
) -> Result<(), Error> {
    let lower = name.to_ascii_lowercase();
    let Some(idef) = cat.indexes.get(&lower) else {
        if if_exists {
            return Ok(());
        }
        return Err(Error::NoSuchTable);
    };
    if idef.auto {
        return Err(Error::Misuse);
    }
    let idef = cat.indexes.remove(&lower).ok_or(Error::NoSuchTable)?;
    cat.index_order.retain(|x| x != &lower);
    Tree::open(idef.root).destroy(pager)?;
    Tree::open(catalog_root).remove(pager, idef.name.as_bytes())?;
    Ok(())
}
