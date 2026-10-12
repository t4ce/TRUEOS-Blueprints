use crate::column::ColumnIndex;
use crate::error::Error;
use crate::ext::ustr::UStr;
use crate::{Sqlite, SqliteArguments, SqliteColumn, SqliteTypeInfo};
use sqlx_core::sql_str::SqlStr;
use sqlx_core::{Either, HashMap};
use std::sync::Arc;

pub(crate) use sqlx_core::statement::*;

#[cfg(not(any(target_os = "trueos", feature = "trueos-remote")))]
mod handle;

#[cfg(feature = "unlock-notify")]
#[cfg(not(any(target_os = "trueos", feature = "trueos-remote")))]
pub(super) mod unlock_notify;
#[cfg(not(any(target_os = "trueos", feature = "trueos-remote")))]
mod r#virtual;

#[cfg(not(any(target_os = "trueos", feature = "trueos-remote")))]
pub(crate) use handle::StatementHandle;
#[cfg(not(any(target_os = "trueos", feature = "trueos-remote")))]
pub(crate) use r#virtual::VirtualStatement;

#[derive(Debug, Clone)]
#[allow(clippy::rc_buffer)]
pub struct SqliteStatement {
    pub(crate) sql: SqlStr,
    pub(crate) parameters: usize,
    pub(crate) columns: Arc<Vec<SqliteColumn>>,
    pub(crate) column_names: Arc<HashMap<UStr, usize>>,
}

impl Statement for SqliteStatement {
    type Database = Sqlite;

    fn into_sql(self) -> SqlStr {
        self.sql
    }

    fn sql(&self) -> &SqlStr {
        &self.sql
    }

    fn parameters(&self) -> Option<Either<&[SqliteTypeInfo], usize>> {
        Some(Either::Right(self.parameters))
    }

    fn columns(&self) -> &[SqliteColumn] {
        &self.columns
    }

    impl_statement_query!(SqliteArguments);
}

impl ColumnIndex<SqliteStatement> for &'_ str {
    fn index(&self, statement: &SqliteStatement) -> Result<usize, Error> {
        statement
            .column_names
            .get(*self)
            .ok_or_else(|| Error::ColumnNotFound((*self).into()))
            .copied()
    }
}
