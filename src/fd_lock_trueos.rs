//! Advisory locks delegate to the owning native file object.
use std::fs::{File, TryLockError};
use std::io;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

pub trait AsOpenFile {
    fn as_open_file(&self) -> &File;
}
impl AsOpenFile for File {
    fn as_open_file(&self) -> &File {
        self
    }
}
impl<T: AsOpenFile + ?Sized> AsOpenFile for &T {
    fn as_open_file(&self) -> &File {
        (**self).as_open_file()
    }
}
impl<T: AsOpenFile + ?Sized> AsOpenFile for &mut T {
    fn as_open_file(&self) -> &File {
        (**self).as_open_file()
    }
}
impl<T: AsOpenFile + ?Sized> AsOpenFile for Box<T> {
    fn as_open_file(&self) -> &File {
        (**self).as_open_file()
    }
}
impl<T: AsOpenFile + ?Sized> AsOpenFile for Arc<T> {
    fn as_open_file(&self) -> &File {
        (**self).as_open_file()
    }
}

fn try_error(error: TryLockError) -> io::Error {
    match error {
        TryLockError::WouldBlock => io::ErrorKind::WouldBlock.into(),
        TryLockError::Error(error) => error,
    }
}

#[derive(Debug)]
pub struct RwLock<T: AsOpenFile> {
    inner: T,
}
impl<T: AsOpenFile> RwLock<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
    pub fn into_inner(self) -> T {
        self.inner
    }
    pub fn read(&self) -> io::Result<RwLockReadGuard<'_, T>> {
        self.inner.as_open_file().lock_shared()?;
        Ok(RwLockReadGuard { lock: self })
    }
    pub fn try_read(&self) -> io::Result<RwLockReadGuard<'_, T>> {
        self.inner
            .as_open_file()
            .try_lock_shared()
            .map_err(try_error)?;
        Ok(RwLockReadGuard { lock: self })
    }
    pub fn write(&mut self) -> io::Result<RwLockWriteGuard<'_, T>> {
        self.inner.as_open_file().lock()?;
        Ok(RwLockWriteGuard { lock: self })
    }
    pub fn try_write(&mut self) -> io::Result<RwLockWriteGuard<'_, T>> {
        self.inner.as_open_file().try_lock().map_err(try_error)?;
        Ok(RwLockWriteGuard { lock: self })
    }
}

#[derive(Debug)]
pub struct RwLockReadGuard<'a, T: AsOpenFile> {
    lock: &'a RwLock<T>,
}
impl<T: AsOpenFile> Deref for RwLockReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.lock.inner
    }
}
impl<T: AsOpenFile> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        let _ = self.lock.inner.as_open_file().unlock();
    }
}

#[derive(Debug)]
pub struct RwLockWriteGuard<'a, T: AsOpenFile> {
    lock: &'a mut RwLock<T>,
}
impl<T: AsOpenFile> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.lock.inner
    }
}
impl<T: AsOpenFile> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.lock.inner
    }
}
impl<T: AsOpenFile> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        let _ = self.lock.inner.as_open_file().unlock();
    }
}
