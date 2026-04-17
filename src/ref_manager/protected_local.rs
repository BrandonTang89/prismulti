use std::cell::{Cell, UnsafeCell};
use sylvan_sys::MTBDD;
use sylvan_sys::mtbdd::{Sylvan_mtbdd_protect, Sylvan_mtbdd_unprotect};

/// A stack-local protected MTBDD slot.
///
/// Keep this value at a stable address while it is alive. Creating the wrapper
/// registers the slot with Sylvan's local protection stack; dropping it
/// unregisters the slot.
#[derive(Debug)]
pub struct ProtectedLocal {
    slot: UnsafeCell<MTBDD>,
    protected: Cell<bool>,
}

impl ProtectedLocal {
    pub fn new(initial: MTBDD) -> Self {
        Self {
            slot: UnsafeCell::new(initial),
            protected: Cell::new(false),
        }
    }

    #[inline]
    fn ensure_protected(&self) {
        if self.protected.get() {
            return;
        }

        unsafe {
            Sylvan_mtbdd_protect(self.slot.get());
        }
        self.protected.set(true);
    }

    #[inline]
    pub fn get(&self) -> MTBDD {
        self.ensure_protected();
        unsafe { *self.slot.get() }
    }

    #[inline]
    pub fn set(&mut self, value: MTBDD) {
        self.ensure_protected();
        unsafe {
            *self.slot.get() = value;
        }
    }

    #[inline]
    pub fn replace(&mut self, value: MTBDD) -> MTBDD {
        self.ensure_protected();
        unsafe { std::mem::replace(&mut *self.slot.get(), value) }
    }

    #[inline]
    pub fn as_ptr(&self) -> *const MTBDD {
        self.ensure_protected();
        self.slot.get() as *const MTBDD
    }

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut MTBDD {
        self.ensure_protected();
        self.slot.get()
    }
}

impl Drop for ProtectedLocal {
    fn drop(&mut self) {
        if !self.protected.get() {
            return;
        }
        unsafe {
            Sylvan_mtbdd_unprotect(self.slot.get());
        }
    }
}

#[derive(Debug)]
pub struct ProtectedBddLocal {
    local: ProtectedLocal,
}

impl ProtectedBddLocal {
    pub fn new(initial: super::BddNode) -> Self {
        Self {
            local: ProtectedLocal::new(initial.0),
        }
    }

    #[inline]
    pub fn get(&self) -> super::BddNode {
        super::BddNode(self.local.get())
    }

    #[inline]
    pub fn set(&mut self, value: super::BddNode) {
        self.local.set(value.0);
    }

    #[inline]
    pub fn replace(&mut self, value: super::BddNode) -> super::BddNode {
        super::BddNode(self.local.replace(value.0))
    }
}

#[derive(Debug)]
pub struct ProtectedMapLocal {
    local: ProtectedLocal,
}

impl ProtectedMapLocal {
    pub fn new(initial: super::BddMap) -> Self {
        Self {
            local: ProtectedLocal::new(initial.0),
        }
    }

    #[inline]
    pub fn get(&self) -> super::BddMap {
        super::BddMap(self.local.get())
    }

    #[inline]
    pub fn set(&mut self, value: super::BddMap) {
        self.local.set(value.0);
    }

    #[inline]
    pub fn replace(&mut self, value: super::BddMap) -> super::BddMap {
        super::BddMap(self.local.replace(value.0))
    }
}

#[derive(Debug)]
pub struct ProtectedVarSetLocal {
    local: ProtectedLocal,
}

impl ProtectedVarSetLocal {
    pub fn new(initial: super::VarSet) -> Self {
        Self {
            local: ProtectedLocal::new(initial.0),
        }
    }

    #[inline]
    pub fn get(&self) -> super::VarSet {
        super::VarSet(self.local.get())
    }

    #[inline]
    pub fn set(&mut self, value: super::VarSet) {
        self.local.set(value.0);
    }

    #[inline]
    pub fn replace(&mut self, value: super::VarSet) -> super::VarSet {
        super::VarSet(self.local.replace(value.0))
    }
}

#[derive(Debug)]
pub struct ProtectedAddLocal {
    local: ProtectedLocal,
}

impl ProtectedAddLocal {
    pub fn new(initial: super::AddNode) -> Self {
        Self {
            local: ProtectedLocal::new(initial.0),
        }
    }

    #[inline]
    pub fn get(&self) -> super::AddNode {
        super::AddNode(self.local.get())
    }

    #[inline]
    pub fn set(&mut self, value: super::AddNode) {
        self.local.set(value.0);
    }

    #[inline]
    pub fn replace(&mut self, value: super::AddNode) -> super::AddNode {
        super::AddNode(self.local.replace(value.0))
    }
}
