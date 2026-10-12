pub(crate) use sqlx_core::error::*;
use std::{
    borrow::Cow,
    error::Error as StdError,
    fmt::{self, Display, Formatter},
};
#[derive(Debug)]
pub struct SqliteError {
    pub(crate) code: i32,
    pub(crate) message: Cow<'static, str>,
}
impl Display for SqliteError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // We include the code as some produce ambiguous messages:
        // SQLITE_BUSY: "database is locked"
        // SQLITE_LOCKED: "database table is locked"
        // Sadly there's no function to get the string label back from an error code.
        write!(f, "(code: {}) {}", self.code, self.message)
    }
}

impl StdError for SqliteError {}

impl DatabaseError for SqliteError {
    #[inline]
    fn message(&self) -> &str {
        &self.message
    }

    /// The extended result code.
    #[inline]
    fn code(&self) -> Option<Cow<'_, str>> {
        Some(format!("{}", self.code).into())
    }

    #[doc(hidden)]
    fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
        self
    }

    #[doc(hidden)]
    fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
        self
    }

    #[doc(hidden)]
    fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
        self
    }

    fn kind(&self) -> ErrorKind {
        match self.code {
            2067 | 1555 => ErrorKind::UniqueViolation,
            787 => ErrorKind::ForeignKeyViolation,
            1299 => ErrorKind::NotNullViolation,
            275 => ErrorKind::CheckViolation,
            _ => ErrorKind::Other,
        }
    }
}
