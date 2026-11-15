//! Top-level SQL database handle managing transactions, catalog, and execution.

use crate::sql::ast::Stmt;
use crate::sql::catalog::{
    exec_create_index, exec_create_table, exec_drop_index, exec_drop_table, Catalog,
};
use crate::sql::dml::{exec_delete, exec_insert, exec_update};
use crate::sql::parser::parse_statement;
use crate::sql::select::{exec_explain, exec_select};
use crate::sql::Rows;
use crate::{Error, Pager, Storage, Tree};

/// Transactional SQL database backed by a Iridium pager and B+trees.
#[derive(Debug)]
pub struct Db<S: Storage> {
    pager: Pager<S>,
    catalog_root: u64,
    catalog: Catalog,
    in_tx: bool,
}

impl<S: Storage> Db<S> {
    /// Open or initialize a SQL database over `main` and `log` storage.
    pub fn open(main: S, log: S) -> Result<Db<S>, Error> {
        let mut pager = Pager::open(main, log)?;
        let catalog_root = if pager.root() == 0 {
            pager.begin()?;
            let tree = Tree::create(&mut pager)?;
            let root = tree.root();
            pager.set_root(root)?;
            pager.commit()?;
            root
        } else {
            pager.root()
        };
        let catalog = Catalog::load(&mut pager, catalog_root)?;
        Ok(Db {
            pager,
            catalog_root,
            catalog,
            in_tx: false,
        })
    }

    /// Execute a single SQL statement (with optional trailing `;`) and return its rows or change count.
    pub fn execute(&mut self, sql: &str) -> Result<Rows, Error> {
        let stmt = parse_statement(sql)?;
        match stmt {
            Stmt::Begin => {
                if self.in_tx {
                    return Err(Error::Misuse);
                }
                self.pager.begin()?;
                self.in_tx = true;
                Ok(Rows::default())
            }
            Stmt::Commit => {
                if !self.in_tx {
                    return Err(Error::Misuse);
                }
                self.pager.commit()?;
                self.in_tx = false;
                Ok(Rows::default())
            }
            Stmt::Rollback => {
                if !self.in_tx {
                    return Err(Error::Misuse);
                }
                self.pager.rollback();
                self.in_tx = false;
                self.catalog = Catalog::load(&mut self.pager, self.catalog_root)?;
                Ok(Rows::default())
            }
            Stmt::Explain(ref sel) => exec_explain(&self.catalog, sel, sql),
            Stmt::Select(ref sel) => exec_select(&mut self.pager, &self.catalog, sel, sql),
            other => self.exec_mutating(other, sql),
        }
    }

    /// Return whether an explicit SQL transaction (`BEGIN`) is currently active.
    pub fn in_transaction(&self) -> bool {
        self.in_tx
    }

    /// Roll back any uncommitted transaction, checkpoint the log, and return `(main, log)`.
    pub fn close(self) -> Result<(S, S), Error> {
        self.pager.close()
    }

    fn exec_mutating(&mut self, stmt: Stmt, raw_sql: &str) -> Result<Rows, Error> {
        let auto = !self.in_tx;
        if auto {
            self.pager.begin()?;
        }
        let res = match stmt {
            Stmt::CreateTable(ref ct) => exec_create_table(
                &mut self.pager,
                &mut self.catalog,
                self.catalog_root,
                ct,
                raw_sql,
            )
            .map(|()| Rows::default()),
            Stmt::DropTable {
                ref name,
                if_exists,
            } => exec_drop_table(
                &mut self.pager,
                &mut self.catalog,
                self.catalog_root,
                name,
                if_exists,
            )
            .map(|()| Rows::default()),
            Stmt::CreateIndex(ref ci) => exec_create_index(
                &mut self.pager,
                &mut self.catalog,
                self.catalog_root,
                ci,
                raw_sql,
            )
            .map(|()| Rows::default()),
            Stmt::DropIndex {
                ref name,
                if_exists,
            } => exec_drop_index(
                &mut self.pager,
                &mut self.catalog,
                self.catalog_root,
                name,
                if_exists,
            )
            .map(|()| Rows::default()),
            Stmt::Insert(ref ins) => exec_insert(&mut self.pager, &mut self.catalog, ins),
            Stmt::Update(ref upd) => exec_update(&mut self.pager, &mut self.catalog, upd),
            Stmt::Delete(ref del) => exec_delete(&mut self.pager, &mut self.catalog, del),
            _ => unreachable!(),
        };
        match res {
            Ok(rows) => {
                if auto {
                    self.pager.commit()?;
                }
                Ok(rows)
            }
            Err(e) => {
                if auto {
                    self.pager.rollback();
                    self.catalog = Catalog::load(&mut self.pager, self.catalog_root)?;
                }
                Err(e)
            }
        }
    }
}
