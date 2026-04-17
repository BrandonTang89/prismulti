use sylvan_sys::MTBDD;
use sylvan_sys::mtbdd::{Sylvan_mtbdd_protect, Sylvan_mtbdd_unprotect};

/// A stack-local protected MTBDD slot.
///
/// Keep this value at a stable address while it is alive. Construct this wrapper
/// first, then call `protect` once in the final local slot.
#[derive(Debug)]
pub struct ProtectedLocal {
    slot: MTBDD,
}

impl ProtectedLocal {
    pub fn new(initial: MTBDD) -> Self {
        Self { slot: initial }
    }

    #[inline]
    pub fn protect(&mut self) {
        unsafe {
            Sylvan_mtbdd_protect(&mut self.slot as *mut MTBDD);
        }
    }

    #[inline]
    pub fn get(&self) -> MTBDD {
        self.slot
    }

    #[inline]
    pub fn set(&mut self, value: MTBDD) {
        self.slot = value;
    }

    #[inline]
    pub fn replace(&mut self, value: MTBDD) -> MTBDD {
        std::mem::replace(&mut self.slot, value)
    }
}

impl Drop for ProtectedLocal {
    fn drop(&mut self) {
        unsafe {
            Sylvan_mtbdd_unprotect(&mut self.slot as *mut MTBDD);
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
    pub fn protect(&mut self) {
        self.local.protect();
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
    pub fn protect(&mut self) {
        self.local.protect();
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
    pub fn protect(&mut self) {
        self.local.protect();
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
    pub fn protect(&mut self) {
        self.local.protect();
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

#[macro_export]
macro_rules! protected_bdd {
    ($name:ident, $expr:expr) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedBddLocal::new($expr);
        $name.protect();
    };
}

#[macro_export]
macro_rules! protected_add {
    ($name:ident, $expr:expr) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedAddLocal::new($expr);
        $name.protect();
    };
}

#[macro_export]
macro_rules! protected_map {
    ($name:ident, $expr:expr) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedMapLocal::new($expr);
        $name.protect();
    };
}

#[macro_export]
macro_rules! protected_var_set {
    ($name:ident, $expr:expr) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedVarSetLocal::new($expr);
        $name.protect();
    };
}
