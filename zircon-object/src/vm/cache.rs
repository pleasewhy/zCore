use super::check_aligned;
use core::fmt::{Debug, Formatter, Result};
use core::ops::{Deref, DerefMut};

///
pub const CACHE_LINE_SIZE: usize = 64;

///
#[repr(align(64))]
pub struct AlignCacheLine<T>(T);

impl<T> AlignCacheLine<T> {
    ///
    pub const fn new(t: T) -> Self {
        Self(t)
    }
}

impl<T: Copy> AlignCacheLine<T> {
    ///
    pub fn get(&self) -> T {
        self.0
    }
}

impl<T> Deref for AlignCacheLine<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for AlignCacheLine<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> AsRef<T> for AlignCacheLine<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T> AsMut<T> for AlignCacheLine<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> From<T> for AlignCacheLine<T> {
    fn from(t: T) -> Self {
        Self::new(t)
    }
}

impl<T: Debug> Debug for AlignCacheLine<T> {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.0.fmt(f)
    }
}

/// Check whether `x` is a multiple of `CACHE_LINE_SIZE`.
pub fn cache_aligned(x: usize) -> bool {
    check_aligned(x, CACHE_LINE_SIZE)
}

/// Round up `size` to a multiple of `CACHE_LINE_SIZE`.
pub fn roundup_cache(size: usize) -> usize {
    (size + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1)
}
