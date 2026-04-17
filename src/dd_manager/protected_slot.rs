use sylvan_sys::MTBDD;
use sylvan_sys::mtbdd::{Sylvan_mtbdd_protect, Sylvan_mtbdd_unprotect};

use crate::dd_manager::dd;

#[derive(Debug)]
pub struct ProtectedSlot {
    slot: Box<MTBDD>,
}

impl ProtectedSlot {
    /// Allocates a protected heap slot for one Sylvan node.
    pub fn new(initial: MTBDD) -> Self {
        let mut slot = Box::new(initial);
        unsafe {
            Sylvan_mtbdd_protect(&mut *slot as *mut MTBDD);
        }
        Self { slot }
    }

    #[inline]
    /// Returns the current node value.
    pub fn get(&self) -> MTBDD {
        *self.slot
    }

    #[inline]
    /// Updates the node value in place.
    pub fn set(&mut self, value: MTBDD) {
        *self.slot = value;
    }

    #[inline]
    /// Replaces the current value and returns the previous node.
    pub fn replace(&mut self, value: MTBDD) -> MTBDD {
        std::mem::replace(&mut *self.slot, value)
    }

    #[inline]
    /// Returns a raw const pointer to the protected slot.
    pub fn as_ptr(&self) -> *const MTBDD {
        &*self.slot as *const MTBDD
    }

    #[inline]
    /// Returns a raw mutable pointer to the protected slot.
    pub fn as_mut_ptr(&mut self) -> *mut MTBDD {
        &mut *self.slot as *mut MTBDD
    }
}

impl Drop for ProtectedSlot {
    fn drop(&mut self) {
        unsafe {
            Sylvan_mtbdd_unprotect(&mut *self.slot as *mut MTBDD);
        }
    }
}

#[derive(Debug)]
pub struct ProtectedBddSlot {
    slot: ProtectedSlot,
}

impl ProtectedBddSlot {
    /// Creates a protected slot for a BDD node.
    pub fn new(initial: super::BddNode) -> Self {
        Self {
            slot: ProtectedSlot::new(initial.0),
        }
    }

    #[inline]
    /// Returns the current BDD value.
    pub fn get(&self) -> super::BddNode {
        super::BddNode(self.slot.get())
    }

    #[inline]
    /// Updates the BDD value.
    pub fn set(&mut self, value: super::BddNode) {
        self.slot.set(value.0);
    }

    #[inline]
    /// Replaces and returns the previous BDD value.
    pub fn replace(&mut self, value: super::BddNode) -> super::BddNode {
        super::BddNode(self.slot.replace(value.0))
    }
}

#[derive(Debug)]
pub struct ProtectedVarSetSlot {
    slot: ProtectedSlot,
}

impl ProtectedVarSetSlot {
    /// Creates a protected slot for a variable set.
    pub fn new(initial: super::VarSet) -> Self {
        Self {
            slot: ProtectedSlot::new(initial.0),
        }
    }

    #[inline]
    /// Returns the current variable set.
    pub fn get(&self) -> super::VarSet {
        super::VarSet(self.slot.get())
    }

    #[inline]
    /// Updates the variable set value.
    pub fn set(&mut self, value: super::VarSet) {
        self.slot.set(value.0);
    }

    #[inline]
    /// Replaces and returns the previous variable set.
    pub fn replace(&mut self, value: super::VarSet) -> super::VarSet {
        super::VarSet(self.slot.replace(value.0))
    }
}

impl Default for ProtectedVarSetSlot {
    fn default() -> Self {
        Self::new(dd::var_set_empty())
    }
}

#[derive(Debug)]
pub struct ProtectedAddSlot {
    slot: ProtectedSlot,
}

impl ProtectedAddSlot {
    /// Creates a protected slot for an ADD node.
    pub fn new(initial: super::AddNode) -> Self {
        Self {
            slot: ProtectedSlot::new(initial.0),
        }
    }

    #[inline]
    /// Returns the current ADD value.
    pub fn get(&self) -> super::AddNode {
        super::AddNode(self.slot.get())
    }

    #[inline]
    /// Updates the ADD value.
    pub fn set(&mut self, value: super::AddNode) {
        self.slot.set(value.0);
    }

    #[inline]
    /// Replaces and returns the previous ADD value.
    pub fn replace(&mut self, value: super::AddNode) -> super::AddNode {
        super::AddNode(self.slot.replace(value.0))
    }
}

impl Default for ProtectedAddSlot {
    fn default() -> Self {
        Self::new(dd::add_zero())
    }
}

#[derive(Debug)]
pub struct ProtectedMapSlot {
    slot: ProtectedSlot,
}

impl ProtectedMapSlot {
    /// Creates a protected slot for a substitution map.
    pub fn new(initial: super::BddMap) -> Self {
        Self {
            slot: ProtectedSlot::new(initial.0),
        }
    }

    #[inline]
    /// Returns the current substitution map.
    pub fn get(&self) -> super::BddMap {
        super::BddMap(self.slot.get())
    }

    #[inline]
    /// Updates the substitution map value.
    pub fn set(&mut self, value: super::BddMap) {
        self.slot.set(value.0);
    }

    #[inline]
    /// Replaces and returns the previous substitution map.
    pub fn replace(&mut self, value: super::BddMap) -> super::BddMap {
        super::BddMap(self.slot.replace(value.0))
    }
}

impl Default for ProtectedMapSlot {
    fn default() -> Self {
        Self::new(dd::bdd_map_empty())
    }
}
