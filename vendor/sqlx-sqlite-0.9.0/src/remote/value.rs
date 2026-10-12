//! Owned wire values; SQLite pointers never cross the Blueprint boundary.
use crate::type_info::DataType;
use crate::{Sqlite, SqliteError, SqliteTypeInfo};
pub(crate) use sqlx_core::value::{Value, ValueRef};
use std::{borrow::Cow, str};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(crate) enum WireValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Clone)]
pub struct SqliteValue {
    pub(crate) wire: WireValue,
    // SQLite's string conversion is materialized once, so borrowed decodes are stable.
    bytes: Vec<u8>,
    pub(crate) column_type: SqliteTypeInfo,
}
pub struct SqliteValueRef<'r>(&'r SqliteValue);
#[derive(Debug, thiserror::Error)]
#[error("invalid remote SQLite value conversion")]
pub(crate) struct BorrowedBlobError;

impl SqliteValue {
    pub(crate) fn new(wire: WireValue, column_type: SqliteTypeInfo) -> Self {
        let bytes = match &wire {
            WireValue::Null => vec![],
            WireValue::Integer(v) => v.to_string().into_bytes(),
            WireValue::Real(v) => v.to_string().into_bytes(),
            WireValue::Text(v) => v.as_bytes().to_vec(),
            WireValue::Blob(v) => v.clone(),
        };
        Self {
            wire,
            bytes,
            column_type,
        }
    }
    pub fn reset_borrow(&mut self) {}
    pub fn try_clone(&self) -> Result<Self, SqliteError> {
        Ok(self.clone())
    }
    fn info(&self) -> SqliteTypeInfo {
        SqliteTypeInfo(match self.wire {
            WireValue::Null => return self.column_type.clone(),
            WireValue::Integer(_) => DataType::Integer,
            WireValue::Real(_) => DataType::Float,
            WireValue::Text(_) => DataType::Text,
            WireValue::Blob(_) => DataType::Blob,
        })
    }
}
impl Value for SqliteValue {
    type Database = Sqlite;
    fn as_ref(&self) -> SqliteValueRef<'_> {
        SqliteValueRef(self)
    }
    fn type_info(&self) -> Cow<'_, SqliteTypeInfo> {
        Cow::Owned(self.info())
    }
    fn is_null(&self) -> bool {
        matches!(self.wire, WireValue::Null)
    }
}
impl<'r> SqliteValueRef<'r> {
    pub(crate) fn value(value: &'r SqliteValue) -> Self {
        Self(value)
    }
    pub fn try_to_owned(&self) -> Result<SqliteValue, SqliteError> {
        Ok(self.0.clone())
    }
    pub(super) fn int64(&self) -> Result<i64, BorrowedBlobError> {
        match &self.0.wire {
            WireValue::Integer(v) => Ok(*v),
            WireValue::Real(v) => Ok(*v as i64),
            WireValue::Null => Ok(0),
            _ => str::from_utf8(&self.0.bytes)
                .ok()
                .and_then(|v| v.parse().ok())
                .ok_or(BorrowedBlobError),
        }
    }
    pub(super) fn double(&self) -> Result<f64, BorrowedBlobError> {
        match &self.0.wire {
            WireValue::Integer(v) => Ok(*v as f64),
            WireValue::Real(v) => Ok(*v),
            WireValue::Null => Ok(0.0),
            _ => str::from_utf8(&self.0.bytes)
                .ok()
                .and_then(|v| v.parse().ok())
                .ok_or(BorrowedBlobError),
        }
    }
    pub(super) fn blob_borrowed(&self) -> &'r [u8] {
        &self.0.bytes
    }
    pub(super) fn with_temp_blob<R>(&self, op: impl FnOnce(&[u8]) -> R) -> R {
        op(&self.0.bytes)
    }
    pub(super) fn blob_owned(&self) -> Vec<u8> {
        self.0.bytes.clone()
    }
    pub(super) fn text_borrowed(&self) -> Result<&'r str, str::Utf8Error> {
        str::from_utf8(&self.0.bytes)
    }
    pub(super) fn with_temp_text<R>(
        &self,
        op: impl FnOnce(&str) -> R,
    ) -> Result<R, str::Utf8Error> {
        self.text_borrowed().map(op)
    }
    pub(super) fn text_owned(&self) -> Result<String, str::Utf8Error> {
        self.text_borrowed().map(str::to_string)
    }
}
impl<'r> ValueRef<'r> for SqliteValueRef<'r> {
    type Database = Sqlite;
    fn to_owned(&self) -> SqliteValue {
        self.0.clone()
    }
    fn type_info(&self) -> Cow<'_, SqliteTypeInfo> {
        Cow::Owned(self.0.info())
    }
    fn is_null(&self) -> bool {
        self.0.is_null()
    }
}
