//! Parallel iterators with a Tokio task backend.
//!
//! Iterator splitting and collection algorithms are retained from Rayon 1.12.0
//! (MIT OR Apache-2.0). The Rayon worker scheduler is not used. See README.md
//! for the scoped job lifetime and shared runtime contract.

#![deny(missing_debug_implementations)]
#![deny(missing_docs)]

extern crate alloc;

#[macro_use]
mod delegate;

mod split_producer;

pub mod array;
pub mod collections;
pub mod iter;
pub mod option;
pub mod prelude;
pub mod range;
pub mod range_inclusive;
pub mod result;
pub mod slice;
pub mod str;
pub mod string;
pub mod vec;

mod math;
mod par_either;

mod compile_fail;

pub mod runtime;
pub use runtime::{FnContext, ThreadPool, ThreadPoolBuilder, ThreadPoolBuildError,
    Scope, scope, in_place_scope, join, join_context, spawn, current_num_threads,
    current_thread_index};

/// We need to transmit raw pointers across threads. It is possible to do this
/// without any unsafe code by converting pointers to usize or to AtomicPtr<T>
/// then back to a raw pointer for use. We prefer this approach because code
/// that uses this type is more explicit.
///
/// Unsafe code is still required to dereference the pointer, so this type is
/// not unsound on its own, although it does partly lift the unconditional
/// !Send and !Sync on raw pointers. As always, dereference with care.
struct SendPtr<T>(*mut T);

// SAFETY: !Send for raw pointers is not for safety, just as a lint
unsafe impl<T: Send> Send for SendPtr<T> {}

// SAFETY: !Sync for raw pointers is not for safety, just as a lint
unsafe impl<T: Send> Sync for SendPtr<T> {}

impl<T> SendPtr<T> {
    // Helper to avoid disjoint captures of `send_ptr.0`
    fn get(self) -> *mut T {
        self.0
    }
}

// Implement Clone without the T: Clone bound from the derive
impl<T> Clone for SendPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

// Implement Copy without the T: Copy bound from the derive
impl<T> Copy for SendPtr<T> {}
