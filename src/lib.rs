//! This crate gives a generic way to add a callback to any dropping value
//! 
//! You may use this for debugging values, see the [struct documentation](struct.DropGuard.html) or the [standalone examples](https://github.com/dns2utf8/drop_guard/tree/master/examples).
//! 
//! # Example:
//! 
//! ```
//! extern crate drop_guard;
//! 
//! use drop_guard::DropGuard;
//! 
//! use std::thread::{spawn, sleep};
//! use std::time::Duration;
//! 
//! fn main() {
//!     // The guard must have a name. `_` will drop it instantly, which would
//!     // lead to unexpected results
//!     let _g = DropGuard::new(spawn(move || {
//!                             sleep(Duration::from_secs(2));
//!                             println!("println! from thread");
//!                         })
//!                         , |join_handle| join_handle.join().unwrap());
//!     
//!     println!("Waiting for thread ...");
//! }
//! ```
//! 


use std::ops::{Deref, DerefMut};
use std::boxed::Box;
use std::mem::ManuallyDrop;
use std::ptr;

/// The DropGuard will remain to `Send` and `Sync` from `T`.
///
/// # Examples
///
/// The `LinkedList<T>` is `Send`.
/// So the `DropGuard` will be too, but it will not be `Sync`:
///
/// ```
/// use drop_guard::DropGuard;
/// use std::collections::LinkedList;
/// use std::thread;
///
/// let list: LinkedList<u32> = LinkedList::new();
///
/// let a_list = DropGuard::new(list, |_| {});
///
/// // Send the guarded list to another thread
/// thread::spawn(move || {
///     assert_eq!(0, a_list.len());
/// }).join();
/// ```
pub struct DropGuard<T, F: FnOnce(T)> {
    data: ManuallyDrop<T>,
    func: ManuallyDrop<Box<F>>,
}

impl<T: Sized, F: FnOnce(T)> DropGuard<T, F> {
    /// Creates a new guard taking in your data and a function.
    /// 
    /// ```
    /// use drop_guard::DropGuard;
    /// 
    /// let s = String::from("a commonString");
    /// let mut s = DropGuard::new(s, |final_string| println!("s became {} at last", final_string));
    /// 
    /// // much code and time passes by ...
    /// *s = "a rainbow".to_string();
    /// 
    /// // by the end of this function the String will have become a rainbow
    /// ```
    pub fn new(data: T, func: F) -> DropGuard<T, F> {
        DropGuard {
            data: ManuallyDrop::new(data),
            func: ManuallyDrop::new(Box::new(func)),
        }
    }
}


/// Use the captured value.
///
/// ```
/// use drop_guard::DropGuard;
///
/// let val = DropGuard::new(42usize, |_| {});
/// assert_eq!(42, *val);
/// ```
impl<T, F: FnOnce(T)> Deref for DropGuard<T, F> {
    type Target = T;

    fn deref(&self) -> &T {
        &*self.data
    }
}

/// Modify the captured value.
///
/// ```
/// use drop_guard::DropGuard;
///
/// let mut val = DropGuard::new(vec![2, 3, 4], |_| {});
/// assert_eq!(3, val.len());
///
/// val.push(5);
/// assert_eq!(4, val.len());
/// ```
impl<T, F: FnOnce(T)> DerefMut for DropGuard<T, F> {
    fn deref_mut(&mut self) -> &mut T {
        &mut*self.data
    }
}

/// React to dropping the value.
/// In this example we measure the time the value is alive.
///
/// ```
/// use drop_guard::DropGuard;
/// use std::time::Instant;
///
/// let start_time = Instant::now();
/// let val = DropGuard::new(42usize, |_| {
///     let time_alive = start_time.elapsed();
///     println!("value lived for {}ns", time_alive.subsec_nanos())
/// });
/// assert_eq!(42, *val);
/// ```
impl<T,F: FnOnce(T)> Drop for DropGuard<T, F> {
    fn drop(&mut self) {
        // Copy the data and guard into local variables.
        // This is OK because the fields are wrapped in `ManuallyDrop`
        // and will not be dropped by the compiler.
        let data: T = unsafe { ptr::read(&*self.data) };
        let func: Box<F> = unsafe { ptr::read(&*self.func) };
        // Run the guard.
        // The call consumes both data and guard and therefore move them out
        // of the local variables. If the guard panics and unwinds,
        // data is dropped by a landing pad inside func.
        func(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::cell::Cell;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    #[test]
    fn it_works() {
        let mut i = 0;
        {
            let _ = DropGuard::new(0, |_| i = 42);
        }
        assert_eq!(42, i);
    }

    #[test]
    fn deref() {
        let g = DropGuard::new(5usize, |_| {});
        assert_eq!(5usize, *g);
    }

    #[test]
    fn deref_mut() {
        let mut g = DropGuard::new(5usize, |_| {});
        *g = 12;
        assert_eq!(12usize, *g);
    }

    #[test]
    fn drop_change() {
        let a = Arc::new(AtomicUsize::new(9));
        {
            let _ = DropGuard::new(a.clone()
                                , |i| i.store(42, Ordering::Relaxed));
        }
        assert_eq!(42usize, a.load(Ordering::Relaxed));
    }

    #[test]
    fn keep_sync_shared_data() {
        fn assert_sync<T: Sync>(_: T) {}
        let g = DropGuard::new(vec![0], |_| {});
        assert_sync(g);
    }

    #[test]
    fn keep_send_shared_data() {
        fn assert_send<T: Send>(_: T) {}
        let g = DropGuard::new(vec![0], |_| {});
        assert_send(g);
    }

    #[test]
    fn guard_runs_on_unwind() {
        let mut was_unwinding = None;
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = DropGuard::new(&mut was_unwinding, |r| {
                *r = Some(std::thread::panicking())
            });
            panic!();
        }));
        assert_eq!(was_unwinding, Some(true));
    }

    #[test]
    fn closure_panics() {
        let data_dropped = Cell::new(0);
        let data = DropGuard::new((), |()| data_dropped.set(data_dropped.get()+1));
        let captured_dropped = Cell::new(0);
        let captured = DropGuard::new((), |()| captured_dropped.set(captured_dropped.get()+1));
        let mut guard_called = 0;
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = DropGuard::new(data, |_data| {
                guard_called += 1;
                let _move = captured;
                panic!();
            });
        }));
        assert_eq!(guard_called, 1);
        assert_eq!(data_dropped.get(), 1);
        assert_eq!(captured_dropped.get(), 1);
    }
}
