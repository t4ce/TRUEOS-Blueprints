use alloc::string::{String, ToString};
use core::borrow::Borrow;
use core::fmt;
use core::ops::Deref;

#[repr(transparent)]
pub struct Path {
    inner: str,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PathBuf {
    inner: String,
}

pub struct Display<'a> {
    path: &'a Path,
}

impl Path {
    pub fn new<S: AsRef<str> + ?Sized>(path: &S) -> &Self {
        unsafe { &*(path.as_ref() as *const str as *const Self) }
    }

    pub fn to_str(&self) -> Option<&str> {
        Some(&self.inner)
    }

    pub fn as_str(&self) -> &str {
        &self.inner
    }

    pub fn to_path_buf(&self) -> PathBuf {
        self.to_owned()
    }

    pub fn parent(&self) -> Option<&Path> {
        let trimmed = trim_trailing_slashes(self.as_str());
        if trimmed.is_empty() {
            return None;
        }
        let index = trimmed.rfind('/')?;
        if index == 0 {
            Some(Path::new("/"))
        } else {
            Some(Path::new(&trimmed[..index]))
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        let trimmed = trim_trailing_slashes(self.as_str());
        let name = trimmed.rsplit('/').next()?;
        if name.is_empty() { None } else { Some(name) }
    }

    pub fn join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        let path = path.as_ref();
        if path.as_str().starts_with('/') {
            return PathBuf::from(path.as_str());
        }

        let mut joined = PathBuf::from(self.as_str());
        joined.push(path);
        joined
    }

    pub fn display(&self) -> Display<'_> {
        Display { path: self }
    }
}

impl PathBuf {
    pub fn new() -> Self {
        Self {
            inner: String::new(),
        }
    }

    pub fn as_path(&self) -> &Path {
        Path::new(self.inner.as_str())
    }

    pub fn display(&self) -> Display<'_> {
        self.as_path().display()
    }

    pub fn push<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();
        if path.as_str().starts_with('/') {
            self.inner.clear();
            self.inner.push_str(path.as_str());
            return;
        }

        if !self.inner.is_empty() && !self.inner.ends_with('/') {
            self.inner.push('/');
        }
        self.inner.push_str(path.as_str());
    }
}

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<Path> for PathBuf {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl AsRef<Path> for String {
    fn as_ref(&self) -> &Path {
        Path::new(self.as_str())
    }
}

impl Borrow<Path> for PathBuf {
    fn borrow(&self) -> &Path {
        self.as_path()
    }
}

impl Deref for PathBuf {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.as_path()
    }
}

impl From<String> for PathBuf {
    fn from(value: String) -> Self {
        Self { inner: value }
    }
}

impl From<&str> for PathBuf {
    fn from(value: &str) -> Self {
        Self {
            inner: value.to_string(),
        }
    }
}

impl From<&Path> for PathBuf {
    fn from(value: &Path) -> Self {
        value.to_path_buf()
    }
}

impl From<&PathBuf> for PathBuf {
    fn from(value: &PathBuf) -> Self {
        value.clone()
    }
}

impl ToOwned for Path {
    type Owned = PathBuf;

    fn to_owned(&self) -> Self::Owned {
        PathBuf::from(self.as_str())
    }
}

impl fmt::Debug for Path {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), formatter)
    }
}

impl fmt::Display for Display<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.path.as_str())
    }
}

fn trim_trailing_slashes(path: &str) -> &str {
    if path.len() > 1 {
        path.trim_end_matches('/')
    } else {
        path
    }
}
