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
    /// Creates a stack-local slot without registering protection yet.
    pub fn new(initial: MTBDD) -> Self {
        Self { slot: initial }
    }

    #[inline]
    /// Registers this local slot with Sylvan protection.
    pub fn protect(&mut self) {
        unsafe {
            Sylvan_mtbdd_protect(&mut self.slot as *mut MTBDD);
        }
    }

    #[inline]
    /// Returns the current node value.
    pub fn get(&self) -> MTBDD {
        self.slot
    }

    #[inline]
    /// Updates the local node value.
    pub fn set(&mut self, value: MTBDD) {
        self.slot = value;
    }

    #[inline]
    /// Replaces and returns the previous node value.
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
    /// __Do not call this directly__ \
    /// Use the `protected_bdd!` macro to create a new protected BDD slot
    pub fn new(initial: super::BddNode) -> Self {
        Self {
            local: ProtectedLocal::new(initial.0),
        }
    }

    #[inline]
    /// Registers this local BDD slot with Sylvan protection.
    pub fn protect(&mut self) {
        self.local.protect();
    }

    #[inline]
    /// Returns the current BDD value.
    pub fn get(&self) -> super::BddNode {
        super::BddNode(self.local.get())
    }

    #[inline]
    /// Updates the local BDD value.
    pub fn set(&mut self, value: super::BddNode) {
        self.local.set(value.0);
    }

    #[inline]
    /// Replaces and returns the previous BDD value.
    pub fn replace(&mut self, value: super::BddNode) -> super::BddNode {
        super::BddNode(self.local.replace(value.0))
    }
}

#[derive(Debug)]
pub struct ProtectedMapLocal {
    local: ProtectedLocal,
}

impl ProtectedMapLocal {
    /// __Do not call this directly__ \
    /// Use the `protected_map!` macro to create a new protected BDD map slot with automatic protection.
    pub fn new(initial: super::BddMap) -> Self {
        Self {
            local: ProtectedLocal::new(initial.0),
        }
    }

    #[inline]
    /// Registers this local map slot with Sylvan protection.
    pub fn protect(&mut self) {
        self.local.protect();
    }

    #[inline]
    /// Returns the current substitution map value.
    pub fn get(&self) -> super::BddMap {
        super::BddMap(self.local.get())
    }

    #[inline]
    /// Updates the local substitution map value.
    pub fn set(&mut self, value: super::BddMap) {
        self.local.set(value.0);
    }

    #[inline]
    /// Replaces and returns the previous substitution map value.
    pub fn replace(&mut self, value: super::BddMap) -> super::BddMap {
        super::BddMap(self.local.replace(value.0))
    }
}

#[derive(Debug)]
pub struct ProtectedVarSetLocal {
    local: ProtectedLocal,
}

impl ProtectedVarSetLocal {
    /// __Do not call this directly__ \
    /// Use the `protected_var_set!` macro to create a new protected variable set slot
    pub fn new(initial: super::VarSet) -> Self {
        Self {
            local: ProtectedLocal::new(initial.0),
        }
    }

    #[inline]
    /// Registers this local variable-set slot with Sylvan protection.
    pub fn protect(&mut self) {
        self.local.protect();
    }

    #[inline]
    /// Returns the current variable-set value.
    pub fn get(&self) -> super::VarSet {
        super::VarSet(self.local.get())
    }

    #[inline]
    /// Updates the local variable-set value.
    pub fn set(&mut self, value: super::VarSet) {
        self.local.set(value.0);
    }

    #[inline]
    /// Replaces and returns the previous variable-set value.
    pub fn replace(&mut self, value: super::VarSet) -> super::VarSet {
        super::VarSet(self.local.replace(value.0))
    }
}

#[derive(Debug)]
pub struct ProtectedAddLocal {
    local: ProtectedLocal,
}

impl ProtectedAddLocal {
    /// __Do not call this directly__ \
    /// Use the `protected_add!` macro to create a new protected add slot with automatic protection.
    pub fn new(initial: super::AddNode) -> Self {
        Self {
            local: ProtectedLocal::new(initial.0),
        }
    }

    #[inline]
    /// Registers this local ADD slot with Sylvan protection.
    pub fn protect(&mut self) {
        self.local.protect();
    }

    #[inline]
    /// Returns the current ADD value.
    pub fn get(&self) -> super::AddNode {
        super::AddNode(self.local.get())
    }

    #[inline]
    /// Updates the local ADD value.
    pub fn set(&mut self, value: super::AddNode) {
        self.local.set(value.0);
    }

    #[inline]
    /// Replaces and returns the previous ADD value.
    pub fn replace(&mut self, value: super::AddNode) -> super::AddNode {
        super::AddNode(self.local.replace(value.0))
    }
}

/// A stack-local protected BDD slot that automatically protects on construction.
#[macro_export]
macro_rules! protected_bdd {
    ($name:ident, $expr:expr) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedBddLocal::new($expr);
        $name.protect();
    };
    ($name:ident) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedBddLocal::new(
            $crate::dd_manager::dd::bdd_false(),
        );
        $name.protect();
    };
}

/// A stack-local protected MTBDD slot that automatically protects on construction.
#[macro_export]
macro_rules! protected_add {
    ($name:ident, $expr:expr) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedAddLocal::new($expr);
        $name.protect();
    };
    ($name:ident) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedAddLocal::new(
            $crate::dd_manager::dd::add_zero(),
        );
        $name.protect();
    };
}

/// A stack-local protected BDD map slot that automatically protects on construction.
#[macro_export]
macro_rules! protected_map {
    ($name:ident, $expr:expr) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedMapLocal::new($expr);
        $name.protect();
    };
    ($name:ident) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedMapLocal::new(
            $crate::dd_manager::dd::bdd_map_empty(),
        );
        $name.protect();
    };
}

/// A stack-local protected variable set slot that automatically protects on construction.
#[macro_export]
macro_rules! protected_var_set {
    ($name:ident, $expr:expr) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedVarSetLocal::new($expr);
        $name.protect();
    };
    ($name:ident) => {
        #[allow(unused_mut)]
        let mut $name = $crate::dd_manager::protected_local::ProtectedVarSetLocal::new(
            $crate::dd_manager::dd::var_set_empty(),
        );
        $name.protect();
    };
}
