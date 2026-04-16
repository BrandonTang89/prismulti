use std::{marker::PhantomData, rc::Rc};

use sylvan_sys::MTBDD;
use sylvan_sys::mtbdd::{Sylvan_mtbdd_protect, Sylvan_mtbdd_unprotect};

pub trait Protectable {
    fn as_mtbdd_ptr(&mut self) -> *mut MTBDD;
}

impl Protectable for MTBDD {
    fn as_mtbdd_ptr(&mut self) -> *mut MTBDD {
        self as *mut MTBDD
    }
}

impl Protectable for super::BddNode {
    fn as_mtbdd_ptr(&mut self) -> *mut MTBDD {
        &mut self.0 as *mut MTBDD
    }
}

impl Protectable for super::AddNode {
    fn as_mtbdd_ptr(&mut self) -> *mut MTBDD {
        &mut self.0 as *mut MTBDD
    }
}

impl Protectable for super::BddMap {
    fn as_mtbdd_ptr(&mut self) -> *mut MTBDD {
        &mut self.0 as *mut MTBDD
    }
}

impl Protectable for super::VarSet {
    fn as_mtbdd_ptr(&mut self) -> *mut MTBDD {
        &mut self.0 as *mut MTBDD
    }
}

pub struct LocalRootsGuard {
    protected: Vec<*mut MTBDD>,
    // Make it neither Send nor Sync: the Sylvan local ref stack is per-thread.
    _not_send_sync: PhantomData<Rc<()>>,
}

impl LocalRootsGuard {
    pub fn new() -> Self {
        Self {
            protected: Vec::new(),
            _not_send_sync: PhantomData,
        }
    }

    /// Root a local MTBDD variable by address.
    ///
    /// # Safety
    /// - `ptr` must point to a valid `MTBDD` variable.
    /// - That variable must remain alive and at the same address until this guard is dropped.
    pub unsafe fn push_raw(&mut self, ptr: *mut MTBDD) {
        unsafe { Sylvan_mtbdd_protect(ptr) };
        self.protected.push(ptr);
    }

    pub fn protect<T: Protectable>(&mut self, value: &mut T) {
        let ptr = value.as_mtbdd_ptr();
        unsafe { self.push_raw(ptr) };
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.protected.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.protected.is_empty()
    }
}

impl Drop for LocalRootsGuard {
    fn drop(&mut self) {
        while let Some(ptr) = self.protected.pop() {
            unsafe { Sylvan_mtbdd_unprotect(ptr) };
        }
    }
}

#[macro_export]
macro_rules! new_protected {
    ($guard:ident, $name:ident, $expr:expr) => {
        let mut $name = $expr;
        $guard.protect(&mut $name);
    };
}
