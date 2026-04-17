//! Runtime ownership/types for Sylvan-backed DD operations.

pub mod dd;
pub mod protected_local;
pub mod protected_slot;

use std::{
    env,
    sync::{Mutex, MutexGuard, OnceLock},
};

use sylvan_sys::{
    BDD, BDDMAP, BDDSET, BDDVAR as SYLVAN_BDDVAR, MTBDD,
    bdd::{Sylvan_get_granularity, Sylvan_set_granularity},
    common::{Sylvan_init_package, Sylvan_set_limits},
    lace::Lace_start,
    mt::Sylvan_init_mt,
    mtbdd::{Sylvan_init_bdd, Sylvan_init_mtbdd},
};

pub const EPS: f64 = 1e-10;

#[derive(Debug, Clone, Copy)]
pub struct AddStats {
    pub node_count: usize,
    pub terminal_count: usize,
    pub minterms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BddNode(pub BDD);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BddMap(pub BDDMAP);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VarSet(pub BDDSET);

pub type BDDVAR = SYLVAN_BDDVAR;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AddNode(pub MTBDD);

impl BddNode {
    #[inline]
    pub fn regular(self) -> Self {
        if self.is_complemented() {
            Self(unsafe { sylvan_sys::mtbdd::Sylvan_mtbdd_comp(self.0) })
        } else {
            self
        }
    }

    #[inline]
    pub fn is_complemented(self) -> bool {
        unsafe { sylvan_sys::mtbdd::Sylvan_mtbdd_hascomp(self.0) != 0 }
    }
}

#[derive(Default)]
struct SylvanRuntime {
    initialized: bool,
}

pub struct DDManager {
    pub(crate) next_var_index: BDDVAR,
    runtime_guard: Option<MutexGuard<'static, ()>>,
}

fn env_u32(name: &str) -> Option<u32> {
    env::var(name).ok().and_then(|raw| raw.parse::<u32>().ok())
}

fn sylvan_api_mutex() -> &'static Mutex<()> {
    static API_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    API_MUTEX.get_or_init(|| Mutex::new(()))
}

fn lock_sylvan_api() -> MutexGuard<'static, ()> {
    sylvan_api_mutex()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn sylvan_runtime() -> &'static Mutex<SylvanRuntime> {
    static RUNTIME: OnceLock<Mutex<SylvanRuntime>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(SylvanRuntime::default()))
}

fn lock_runtime() -> std::sync::MutexGuard<'static, SylvanRuntime> {
    sylvan_runtime()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn ensure_runtime_started() {
    let mut runtime = lock_runtime();
    if !runtime.initialized {
        let workers = env_u32("PRISM_SYLVAN_WORKERS").unwrap_or(0);
        let memory_cap = env::var("PRISM_SYLVAN_MEMORY_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(1usize << 30);
        let table_ratio = env::var("PRISM_SYLVAN_TABLE_RATIO")
            .ok()
            .and_then(|raw| raw.parse::<i32>().ok())
            .unwrap_or(1);
        let initial_ratio = env::var("PRISM_SYLVAN_INITIAL_RATIO")
            .ok()
            .and_then(|raw| raw.parse::<i32>().ok())
            .unwrap_or(5);
        unsafe {
            Lace_start(workers, 0);
            Sylvan_set_limits(memory_cap, table_ratio, initial_ratio);
            Sylvan_init_package();
            Sylvan_init_mt();
            Sylvan_init_mtbdd();
            Sylvan_init_bdd();
            if let Some(granularity) = env_u32("PRISM_SYLVAN_GRANULARITY")
                && granularity > 0
            {
                Sylvan_set_granularity(granularity as i32);
            }
            let _ = Sylvan_get_granularity();
        }
        runtime.initialized = true;
    }
}

fn release_runtime_manager() {
    // Keep Sylvan/Lace alive for the process lifetime.
}

impl DDManager {
    pub fn new() -> Self {
        let runtime_guard = lock_sylvan_api();
        ensure_runtime_started();
        Self {
            next_var_index: 0,
            runtime_guard: Some(runtime_guard),
        }
    }

    pub fn new_var(&mut self) -> BDDVAR {
        let idx = self.next_var_index;
        self.next_var_index += 1;
        idx
    }

    pub fn var_count(&self) -> usize {
        self.next_var_index as usize
    }
}

impl Default for DDManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DDManager {
    fn drop(&mut self) {
        release_runtime_manager();
        let _ = self.runtime_guard.take();
    }
}

#[cfg(test)]
mod tests {
    use super::protected_local::{ProtectedAddLocal, ProtectedBddLocal, ProtectedVarSetLocal};
    use super::{BddNode, DDManager};
    use crate::dd_manager::dd;

    fn assert_witness_satisfies(root: BddNode, mgr: &mut DDManager, witness: &[i32]) {
        let root_add = ProtectedAddLocal::new(dd::bdd_to_add(mgr, root));
        let value = dd::add_eval_value(mgr, root_add.get(), witness);
        assert_eq!(value, 1.0, "extracted witness must satisfy root BDD");
    }

    #[test]
    fn extract_leftmost_path_handles_non_complemented_root() {
        let mut mgr = DDManager::new();

        let x0_idx = mgr.new_var();
        let x0 = ProtectedBddLocal::new(dd::bdd_var(&mgr, x0_idx));
        assert!(!x0.get().is_complemented());

        let witness =
            dd::extract_leftmost_path_from_bdd(&mgr, x0.get()).expect("x0 should be satisfiable");

        assert_eq!(witness[0], 1, "leftmost witness for x0 must set x0=1");
        assert_witness_satisfies(x0.get(), &mut mgr, &witness);
    }

    #[test]
    fn extract_leftmost_path_handles_complemented_root() {
        let mut mgr = DDManager::new();

        let x0_idx = mgr.new_var();
        let x0 = ProtectedBddLocal::new(dd::bdd_var(&mgr, x0_idx));
        let not_x0 = ProtectedBddLocal::new(dd::bdd_not(&mgr, x0.get()));
        assert!(not_x0.get().is_complemented());

        let witness = dd::extract_leftmost_path_from_bdd(&mgr, not_x0.get())
            .expect("!x0 should be satisfiable");

        assert_eq!(witness[0], 0, "leftmost witness for !x0 must set x0=0");
        assert_witness_satisfies(not_x0.get(), &mut mgr, &witness);
    }

    #[test]
    fn add_max_abstract_takes_max_over_abstracted_var() {
        let mut mgr = DDManager::new();
        let x0 = mgr.new_var();

        let cond = ProtectedBddLocal::new(dd::bdd_var(&mgr, x0));
        let then_branch = ProtectedAddLocal::new(dd::add_const(&mgr, 0.2));
        let else_branch = ProtectedAddLocal::new(dd::add_const(&mgr, 0.7));
        let f = ProtectedAddLocal::new(dd::add_ite(
            &mut mgr,
            cond.get(),
            then_branch.get(),
            else_branch.get(),
        ));

        let vars = ProtectedVarSetLocal::new(dd::var_set_from_indices(&mgr, &[x0]));
        let max_abs = ProtectedAddLocal::new(dd::add_max_abstract(&mgr, f.get(), vars.get()));

        let value = dd::add_value(&mgr, max_abs.get().0)
            .expect("max abstraction over x0 should yield a constant");
        assert!(
            (value - 0.7).abs() < 1e-12,
            "expected max value 0.7, got {value}"
        );
    }

    #[test]
    fn add_min_abstract_takes_min_over_abstracted_var() {
        let mut mgr = DDManager::new();
        let x0 = mgr.new_var();

        let cond = ProtectedBddLocal::new(dd::bdd_var(&mgr, x0));
        let then_branch = ProtectedAddLocal::new(dd::add_const(&mgr, 0.2));
        let else_branch = ProtectedAddLocal::new(dd::add_const(&mgr, 0.7));
        let f = ProtectedAddLocal::new(dd::add_ite(
            &mut mgr,
            cond.get(),
            then_branch.get(),
            else_branch.get(),
        ));

        let vars = ProtectedVarSetLocal::new(dd::var_set_from_indices(&mgr, &[x0]));
        let min_abs = ProtectedAddLocal::new(dd::add_min_abstract(&mgr, f.get(), vars.get()));

        let value = dd::add_value(&mgr, min_abs.get().0)
            .expect("min abstraction over x0 should yield a constant");
        assert!(
            (value - 0.2).abs() < 1e-12,
            "expected min value 0.2, got {value}"
        );
    }
}
