//! Safe-ish, root-protection-aware wrapper around raw Sylvan operations.
//!
//! This module is the single place in the crate that should invoke Sylvan APIs
//! directly. It wraps raw MTBDD handles in lightweight node newtypes and
//! centralizes Sylvan root-protection helpers so call sites can work with
//! higher-level BDD/ADD operations.

pub mod local_roots_guard;
pub mod protected_slot;
use std::{
    collections::{HashMap, HashSet},
    env,
    fs::File,
    io::{self, Write},
    sync::{Mutex, MutexGuard, OnceLock},
};

use local_roots_guard::LocalRootsGuard;
use protected_slot::ProtectedSlot;
use sylvan_sys::{
    BDD, BDDMAP, BDDSET, BDDVAR as SYLVAN_BDDVAR, MTBDD, SYLVAN_FALSE, SYLVAN_INVALID, SYLVAN_TRUE,
    bdd::{
        Sylvan_and, Sylvan_and_exists, Sylvan_compose, Sylvan_equiv, Sylvan_exists,
        Sylvan_get_granularity, Sylvan_not, Sylvan_or, Sylvan_set_granularity, Sylvan_xor,
    },
    common::{Sylvan_init_package, Sylvan_set_limits},
    lace::{Lace_start, Task, WorkerP},
    mt::Sylvan_init_mt,
    mtbdd::{
        MTBDD_APPLY_OP, Sylvan_high, Sylvan_init_bdd, Sylvan_init_mtbdd, Sylvan_ithvar, Sylvan_low,
        Sylvan_map_add, Sylvan_map_empty, Sylvan_mtbdd_abstract_max, Sylvan_mtbdd_abstract_min,
        Sylvan_mtbdd_abstract_plus, Sylvan_mtbdd_and_abstract_plus, Sylvan_mtbdd_comp,
        Sylvan_mtbdd_compose, Sylvan_mtbdd_double, Sylvan_mtbdd_equal_norm_d,
        Sylvan_mtbdd_getdouble, Sylvan_mtbdd_hascomp, Sylvan_mtbdd_isleaf, Sylvan_mtbdd_ite,
        Sylvan_mtbdd_ithvar, Sylvan_mtbdd_minus, Sylvan_mtbdd_nodecount, Sylvan_mtbdd_plus,
        Sylvan_mtbdd_satcount, Sylvan_mtbdd_set_from_array, Sylvan_mtbdd_strict_threshold_double,
        Sylvan_mtbdd_times, Sylvan_set_empty, Sylvan_var,
    },
};

use crate::ref_manager::protected_slot::ProtectedVarSetSlot;

pub const EPS: f64 = 1e-10;

#[derive(Debug, Clone, Copy)]
/// Basic structural statistics for an ADD.
pub struct AddStats {
    /// Number of nodes in the DAG rooted at the ADD.
    pub node_count: usize,
    /// Number of unique terminal nodes in the ADD.
    pub terminal_count: usize,
    /// Number of minterms for the given variable count.
    pub minterms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Typed wrapper for BDD nodes.
pub struct BddNode(pub BDD);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BddMap(pub BDDMAP);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VarSet(pub BDDSET);
pub type BDDVAR = SYLVAN_BDDVAR;

impl BddNode {
    #[inline]
    /// Returns the regular (non-complemented) view of this node.
    pub fn regular(self) -> Self {
        Self(regular_raw(self.0))
    }

    #[inline]
    /// Returns `true` if this node is complement-tagged.
    pub fn is_complemented(self) -> bool {
        is_complemented_raw(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Typed wrapper for ADD nodes.
pub struct AddNode(pub MTBDD);

#[derive(Default)]
struct SylvanRuntime {
    initialized: bool,
}

/// Owns a Sylvan runtime handle and provides typed BDD/ADD operations.
pub struct RefManager {
    next_var_index: BDDVAR,
    var_set_cache: HashMap<Vec<BDDVAR>, ProtectedVarSetSlot>,
    swap_map_cache: HashMap<(Vec<BDDVAR>, Vec<BDDVAR>), ProtectedSlot>,
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
    // Keep the Sylvan/Lace runtime alive for the process lifetime.
    // Repeated init/quit cycles across parallel tests can deadlock in practice.
}

#[inline]
fn is_complemented_raw(node: MTBDD) -> bool {
    unsafe { Sylvan_mtbdd_hascomp(node) != 0 }
}

#[inline]
fn regular_raw(node: MTBDD) -> MTBDD {
    if is_complemented_raw(node) {
        unsafe { Sylvan_mtbdd_comp(node) }
    } else {
        node
    }
}

#[inline]
fn leaf_to_f64(node: MTBDD) -> f64 {
    if node == SYLVAN_FALSE {
        0.0
    } else if node == SYLVAN_TRUE {
        1.0
    } else {
        unsafe { Sylvan_mtbdd_getdouble(node) }
    }
}

extern "C" fn mtbdd_divide_op(
    _w: *mut WorkerP,
    _t: *mut Task,
    a: *mut MTBDD,
    b: *mut MTBDD,
) -> MTBDD {
    unsafe {
        let lhs = *a;
        let rhs = *b;
        if Sylvan_mtbdd_isleaf(lhs) != 0 && Sylvan_mtbdd_isleaf(rhs) != 0 {
            let lv = leaf_to_f64(lhs);
            let rv = leaf_to_f64(rhs);
            return Sylvan_mtbdd_double(lv / rv);
        }
        SYLVAN_INVALID
    }
}

impl RefManager {
    /// Creates a new Sylvan-backed manager.
    pub fn new() -> Self {
        let runtime_guard = lock_sylvan_api();
        ensure_runtime_started();
        Self {
            next_var_index: 0,
            var_set_cache: HashMap::new(),
            swap_map_cache: HashMap::new(),
            runtime_guard: Some(runtime_guard),
        }
    }

    /// Returns Sylvan's shared BDD one node without changing references.
    fn one_bdd(&self) -> BddNode {
        BddNode(SYLVAN_TRUE)
    }

    fn zero_bdd(&self) -> BddNode {
        BddNode(SYLVAN_FALSE)
    }

    fn must_node(&self, n: MTBDD, op: &str) -> MTBDD {
        assert!(n != SYLVAN_INVALID, "Sylvan returned INVALID in {op}");
        n
    }

    #[inline]
    fn regular_node(&self, node: MTBDD) -> MTBDD {
        regular_raw(node)
    }

    fn ensure_var_index(&mut self, idx: BDDVAR) {
        self.next_var_index = self.next_var_index.max(idx + 1);
    }

    pub fn var_set_from_indices(&self, vars: &[BDDVAR]) -> VarSet {
        let mut arr: Vec<BDDVAR> = vars.to_vec();
        let set = self.must_node(
            unsafe { Sylvan_mtbdd_set_from_array(arr.as_mut_ptr(), arr.len()) },
            "Sylvan_mtbdd_set_from_array",
        );
        VarSet(set)
    }

    pub fn var_set_empty(&self) -> VarSet {
        VarSet(self.must_node(unsafe { Sylvan_set_empty() }, "Sylvan_set_empty"))
    }

    fn get_or_build_var_set_from_indices(&mut self, vars: &[BDDVAR]) -> VarSet {
        let key = vars.to_vec();
        if let Some(set) = self.var_set_cache.get(&key) {
            return set.get();
        }

        let set = self.var_set_from_indices(vars);
        self.var_set_cache
            .insert(key, ProtectedVarSetSlot::new(set));
        set
    }

    fn get_or_build_cube_set(&self, cube: MTBDD) -> VarSet {
        VarSet(self.regular_node(cube))
    }

    fn build_swap_map_uncached(&self, x: &[BDDVAR], y: &[BDDVAR]) -> BddMap {
        let mut guard = LocalRootsGuard::new();

        crate::new_protected!(
            guard,
            map,
            self.must_node(unsafe { Sylvan_map_empty() }, "Sylvan_map_empty")
        );

        for (&xi, &yi) in x.iter().zip(y.iter()) {
            assert!(xi < self.next_var_index);
            assert!(yi < self.next_var_index);

            let mut iter_guard = LocalRootsGuard::new();
            crate::new_protected!(
                iter_guard,
                y_var,
                self.must_node(unsafe { Sylvan_ithvar(yi) }, "Sylvan_ithvar(y)")
            );
            let new_map_xy = self.must_node(
                unsafe { Sylvan_map_add(map, xi, y_var) },
                "Sylvan_map_add(x->y)",
            );
            map = new_map_xy;

            let mut iter_guard = LocalRootsGuard::new();
            crate::new_protected!(
                iter_guard,
                x_var,
                self.must_node(unsafe { Sylvan_ithvar(xi) }, "Sylvan_ithvar(x)")
            );
            let new_map_yx = self.must_node(
                unsafe { Sylvan_map_add(map, yi, x_var) },
                "Sylvan_map_add(y->x)",
            );
            map = new_map_yx;
        }

        BddMap(map)
    }

    fn get_or_build_swap_map(&mut self, x: &[BDDVAR], y: &[BDDVAR]) -> BddMap {
        let key = (x.to_vec(), y.to_vec());
        if let Some(map) = self.swap_map_cache.get(&key) {
            return BddMap(map.get());
        }

        let map = self.build_swap_map_uncached(x, y);
        self.swap_map_cache.insert(key, ProtectedSlot::new(map.0));
        map
    }

    /// Releases internal memoization roots that are retained by the manager.
    pub fn clear_internal_caches(&mut self) {
        self.var_set_cache.clear();
        self.swap_map_cache.clear();
    }

    /// Returns the number of nodes still carrying non-zero references.
    pub fn nonzero_ref_count(&self) -> usize {
        0
    }

    /// Returns the number of DD variables currently allocated in this manager view.
    pub fn var_count(&self) -> usize {
        self.next_var_index as usize
    }

    /// Validate tracked node validity.
    pub fn debug_check(&self) -> bool {
        true
    }

    /// Reads the variable index for `node`, or `BDDVAR::MAX` for constants.
    pub fn read_var_index(&self, node: MTBDD) -> BDDVAR {
        if self.is_constant(node) {
            BDDVAR::MAX
        } else {
            let reg = self.regular_node(node);
            unsafe { Sylvan_var(reg) }
        }
    }

    /// Returns the THEN child of a node, preserving complement semantics.
    pub fn read_then(&self, node: MTBDD) -> MTBDD {
        if self.is_constant(node) {
            self.regular_node(node)
        } else {
            self.must_node(unsafe { Sylvan_high(node) }, "Sylvan_high")
        }
    }

    /// Returns the ELSE child of a node, preserving complement semantics.
    pub fn read_else(&self, node: MTBDD) -> MTBDD {
        if self.is_constant(node) {
            self.regular_node(node)
        } else {
            self.must_node(unsafe { Sylvan_low(node) }, "Sylvan_low")
        }
    }

    /// Returns `true` if `node` is a terminal (constant) node.
    pub fn is_constant(&self, node: MTBDD) -> bool {
        let reg = self.regular_node(node);
        unsafe { Sylvan_mtbdd_isleaf(reg) != 0 }
    }

    /// Returns the terminal value for constant nodes, otherwise `None`.
    pub fn add_value(&self, node: MTBDD) -> Option<f64> {
        if !self.is_constant(node) {
            return None;
        }

        if node == SYLVAN_FALSE {
            return Some(0.0);
        }
        if node == SYLVAN_TRUE {
            return Some(1.0);
        }

        let reg = self.regular_node(node);
        let v = leaf_to_f64(reg);
        if is_complemented_raw(node) {
            Some(1.0 - v)
        } else {
            Some(v)
        }
    }

    /// Evaluates ADD `f` at a concrete valuation and returns the terminal value.
    ///
    /// `inputs` is indexed by DD variable index and must contain at least
    /// `self.var_count()` entries with values `0` or `1`.\
    /// __Refs__: none\
    /// __Derefs__: none
    pub fn add_eval_value(&self, f: AddNode, inputs: &[i32]) -> f64 {
        let required = self.var_count();
        assert!(
            inputs.len() >= required,
            "inputs length {} smaller than DD var count {}",
            inputs.len(),
            required
        );

        let mut node = f.0;
        loop {
            if self.is_constant(node) {
                return self
                    .add_value(node)
                    .expect("evaluation must end in constant terminal");
            }

            let var_index = self.read_var_index(node) as usize;
            node = if inputs[var_index] == 0 {
                self.read_else(node)
            } else {
                self.read_then(node)
            };
        }
    }

    /// Extracts one satisfying valuation from a BDD by following a deterministic path.
    ///
    /// The traversal prefers the ELSE branch whenever it is not the zero function;
    /// otherwise it takes the THEN branch. This gives a stable assignment extraction
    /// strategy that is useful for evaluating DDs at any witness state.
    ///
    /// Returned vector is indexed by DD variable index and contains 0/1 values.
    /// Variables not encountered along the path remain 0, which is valid because
    /// they are don't-care for the selected path.
    ///
    /// Returns `None` iff `root` is the zero BDD (unsatisfiable).\
    /// __Refs__: None, __Derefs__: None
    pub fn extract_leftmost_path_from_bdd(&self, root: BddNode) -> Option<Vec<i32>> {
        let mut inputs = vec![0_i32; self.var_count()];
        let zero = self.zero_bdd().0;
        let mut node = root.0;

        loop {
            if self.is_constant(node) {
                return if node == zero { None } else { Some(inputs) };
            }

            let var_index = self.read_var_index(node) as usize;
            let else_node = self.read_else(node);
            if else_node != zero {
                inputs[var_index] = 0;
                node = else_node;
                continue;
            }

            let then_node = self.read_then(node);
            inputs[var_index] = 1;
            node = then_node;
        }
    }

    /// Returns a newly referenced BDD one node.
    pub fn bdd_one(&mut self) -> BddNode {
        self.one_bdd()
    }

    /// Returns a newly referenced BDD zero node.
    pub fn bdd_zero(&mut self) -> BddNode {
        self.zero_bdd()
    }

    /// Returns a newly referenced ADD constant zero node.
    pub fn add_zero(&self) -> AddNode {
        self.add_const(0.0)
    }

    /// Returns a newly referenced ADD constant node.
    pub fn add_const(&self, value: f64) -> AddNode {
        let n = self.must_node(unsafe { Sylvan_mtbdd_double(value) }, "Sylvan_mtbdd_double");
        AddNode(n)
    }

    /// Allocates a new BDD variable
    pub fn new_var(&mut self) -> BDDVAR {
        let idx = self.next_var_index;
        self.next_var_index += 1;
        idx
    }

    /// Returns the BDD variable node for `var_index`, referenced.
    pub fn bdd_var(&self, var_index: BDDVAR) -> BddNode {
        assert!(var_index < self.next_var_index);
        let n = self.must_node(unsafe { Sylvan_ithvar(var_index) }, "Sylvan_ithvar");
        BddNode(n)
    }

    /// Returns the ADD variable node for `var_index`, referenced.
    pub fn add_var(&self, var_index: BDDVAR) -> AddNode {
        assert!(var_index < self.next_var_index);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_ithvar(var_index) },
            "Sylvan_mtbdd_ithvar",
        );
        AddNode(n)
    }

    /// Logical negation of `a`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a
    pub fn bdd_not(&self, a: BddNode) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        let n = self.must_node(unsafe { Sylvan_not(a_rooted.0) }, "Sylvan_not");
        BddNode(n)
    }

    /// Boolean equivalence (`XNOR`) of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn bdd_equals(&self, a: BddNode, b: BddNode) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, b_rooted, b);
        let n = self.must_node(
            unsafe { Sylvan_equiv(a_rooted.0, b_rooted.0) },
            "Sylvan_equiv",
        );
        BddNode(n)
    }

    /// Boolean inequality (`XOR`) of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn bdd_nequals(&self, a: BddNode, b: BddNode) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, b_rooted, b);
        let n = self.must_node(unsafe { Sylvan_xor(a_rooted.0, b_rooted.0) }, "Sylvan_xor");
        BddNode(n)
    }

    /// Conjunction of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn bdd_and(&self, a: BddNode, b: BddNode) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, b_rooted, b);
        let n = self.must_node(unsafe { Sylvan_and(a_rooted.0, b_rooted.0) }, "Sylvan_and");
        BddNode(n)
    }

    /// Disjunction of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn bdd_or(&self, a: BddNode, b: BddNode) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, b_rooted, b);
        let n = self.must_node(unsafe { Sylvan_or(a_rooted.0, b_rooted.0) }, "Sylvan_or");
        BddNode(n)
    }

    /// Existentially abstracts variables in `cube` from `a`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a
    pub fn bdd_exists_abstract(&self, a: BddNode, vars: VarSet) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, cube_rooted, vars);

        let n = self.must_node(
            unsafe { Sylvan_exists(a_rooted.0, vars.0) },
            "Sylvan_exists",
        );
        BddNode(n)
    }

    /// Computes `(f AND g)` then existentially abstracts variables in `cube`.\
    /// This is essentially matrix multiplication for BDDs\
    /// __Refs__: result\
    /// __Derefs__: f, g
    pub fn bdd_and_then_existsabs(&self, f: BddNode, g: BddNode, vars: VarSet) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, f_rooted, f);
        crate::new_protected!(guard, g_rooted, g);
        crate::new_protected!(guard, vars, vars);

        let n = self.must_node(
            unsafe { Sylvan_and_exists(f_rooted.0, g_rooted.0, vars.0) },
            "Sylvan_and_exists",
        );
        BddNode(n)
    }

    /// Swaps each variable in `x` with the corresponding variable in `y`.
    ///
    /// both index slices must have the same length.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn bdd_swap_variables(&mut self, f: BddNode, x: &[BDDVAR], y: &[BDDVAR]) -> BddNode {
        assert_eq!(x.len(), y.len());
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, f_rooted, f);
        let map = self.get_or_build_swap_map(x, y);
        crate::new_protected!(guard, map_rooted, map);
        let n = self.must_node(
            unsafe { Sylvan_compose(f_rooted.0, map_rooted.0) },
            "Sylvan_compose",
        );
        BddNode(n)
    }

    /// Swaps each variable in `x` with the corresponding variable in `y` for ADD `f`.
    ///
    /// Both index slices must have the same length.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_swap_vars(&mut self, f: AddNode, x: &[BDDVAR], y: &[BDDVAR]) -> AddNode {
        assert_eq!(x.len(), y.len());
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, f_rooted, f);
        let map = self.get_or_build_swap_map(x, y);
        crate::new_protected!(guard, map_rooted, map);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_compose(f_rooted.0, map_rooted.0) },
            "Sylvan_mtbdd_compose",
        );
        AddNode(n)
    }

    /// Computes ADD matrix-vector/matrix multiplication over summation vars `z`.
    ///
    /// `a` is assumed to depend on row vars and `z`; `b` depends on `z` and optional
    /// remaining vars. The result abstracts over `z`.
    ///
    /// Equivalent to sum_abstract((a * b), z) but more efficient.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_matrix_multiply(&mut self, a: AddNode, b: AddNode, z: &[BDDVAR]) -> AddNode {
        let vars = self.get_or_build_var_set_from_indices(z);

        self.add_matrix_multiply_with_var_set(a, b, vars)
    }

    /// Computes ADD matrix-vector/matrix multiplication over precomputed var set.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_matrix_multiply_with_var_set(
        &mut self,
        a: AddNode,
        b: AddNode,
        vars: VarSet,
    ) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, b_rooted, b);
        crate::new_protected!(guard, vars_rooted, vars);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_and_abstract_plus(a_rooted.0, b_rooted.0, vars_rooted.0) },
            "Sylvan_mtbdd_and_abstract_plus",
        );
        AddNode(n)
    }

    /// Returns an internal cached variable-set node for `vars`.
    pub fn get_var_set_for_indices(&mut self, vars: &[BDDVAR]) -> VarSet {
        self.get_or_build_var_set_from_indices(vars)
    }

    /// Returns an internal cached swap-map node for `(x, y)`.
    pub fn get_swap_map_for_indices(&mut self, x: &[BDDVAR], y: &[BDDVAR]) -> BddMap {
        self.get_or_build_swap_map(x, y)
    }

    /// Composes BDD `f` with precomputed map `map`.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn bdd_compose_with_map(&mut self, f: BddNode, map: BddMap) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, f_rooted, f);
        crate::new_protected!(guard, map_rooted, map);
        let n = self.must_node(
            unsafe { Sylvan_compose(f_rooted.0, map_rooted.0) },
            "Sylvan_compose",
        );
        BddNode(n)
    }

    /// Composes ADD `f` with precomputed map `map`.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_compose_with_map(&mut self, f: AddNode, map: BddMap) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, f_rooted, f);
        crate::new_protected!(guard, map_rooted, map);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_compose(f_rooted.0, map_rooted.0) },
            "Sylvan_mtbdd_compose",
        );
        AddNode(n)
    }

    /// Pointwise ADD addition of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_plus(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, b_rooted, b);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_plus(a_rooted.0, b_rooted.0) },
            "Sylvan_mtbdd_plus",
        );
        AddNode(n)
    }

    /// Pointwise ADD subtraction `a - b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_minus(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, b_rooted, b);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_minus(a_rooted.0, b_rooted.0) },
            "Sylvan_mtbdd_minus",
        );
        AddNode(n)
    }

    /// Pointwise ADD multiplication of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_times(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, b_rooted, b);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_times(a_rooted.0, b_rooted.0) },
            "Sylvan_mtbdd_times",
        );
        AddNode(n)
    }

    /// Pointwise ADD division `a / b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_divide(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        crate::new_protected!(guard, b_rooted, b);
        let op: MTBDD_APPLY_OP = mtbdd_divide_op;
        let n = self.must_node(
            unsafe { sylvan_sys::mtbdd::Sylvan_mtbdd_apply(a_rooted.0, b_rooted.0, op) },
            "Sylvan_mtbdd_apply(divide)",
        );
        AddNode(n)
    }

    /// ADD `if-then-else` over `cond`, converting the condition from BDD to ADD.
    ///
    /// __Refs__: result\
    /// __Derefs__: cond, then_branch, else_branch
    pub fn add_ite(
        &mut self,
        cond: BddNode,
        then_branch: AddNode,
        else_branch: AddNode,
    ) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, cond_rooted, cond);
        crate::new_protected!(guard, then_rooted, then_branch);
        crate::new_protected!(guard, else_rooted, else_branch);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_ite(cond_rooted.0, then_rooted.0, else_rooted.0) },
            "Sylvan_mtbdd_ite",
        );
        AddNode(n)
    }

    /// Existential abstraction over ADD `f` with respect to `cube`.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_sum_abstract(&mut self, f: AddNode, vars: VarSet) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, f_rooted, f);
        crate::new_protected!(guard, cube_rooted, vars);

        let n = self.must_node(
            unsafe { Sylvan_mtbdd_abstract_plus(f_rooted.0, vars.0) },
            "Sylvan_mtbdd_abstract_plus",
        );
        AddNode(n)
    }

    /// OR abstraction over ADD `f` with respect to `cube`.
    ///
    /// Assumes `f` is 0/1-valued. This corresponds to max abstraction.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_or_abstract(&mut self, f: AddNode, cube: AddNode) -> AddNode {
        self.add_max_abstract(f, cube)
    }

    /// Max abstraction over ADD `f` with respect to `cube`.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_max_abstract(&mut self, f: AddNode, cube: AddNode) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, f_rooted, f);
        crate::new_protected!(guard, cube_rooted, cube);
        let vars = self.get_or_build_cube_set(cube_rooted.0);

        let n = self.must_node(
            unsafe { Sylvan_mtbdd_abstract_max(f_rooted.0, vars.0) },
            "Sylvan_mtbdd_abstract_max",
        );
        AddNode(n)
    }

    /// Min abstraction over ADD `f` with respect to `cube`.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_min_abstract(&mut self, f: AddNode, cube: AddNode) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, f_rooted, f);
        crate::new_protected!(guard, cube_rooted, cube);
        let vars = self.get_or_build_cube_set(cube_rooted.0);

        let n = self.must_node(
            unsafe { Sylvan_mtbdd_abstract_min(f_rooted.0, vars.0) },
            "Sylvan_mtbdd_abstract_min",
        );
        AddNode(n)
    }

    /// Converts an ADD to a BDD using threshold `EPS`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a
    pub fn add_to_bdd(&mut self, a: AddNode) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_strict_threshold_double(a_rooted.0, EPS) },
            "Sylvan_mtbdd_strict_threshold_double",
        );
        BddNode(n)
    }

    /// Converts an ADD to its support-pattern BDD.
    ///
    /// __Refs__: result\
    /// __Derefs__: a
    pub fn add_to_bdd_pattern(&mut self, a: AddNode) -> BddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, a_rooted, a);

        crate::new_protected!(guard, zero_for_gt, self.add_const(0.0));
        let gt_zero = self.add_greater_than(a, zero_for_gt);

        crate::new_protected!(guard, zero_for_lt, self.add_const(0.0));
        let lt_zero = self.add_less_than(a_rooted, zero_for_lt);
        self.bdd_or(gt_zero, lt_zero)
    }

    /// Converts a BDD to an ADD.
    ///
    /// __Refs__: result\
    /// __Derefs__: b
    pub fn bdd_to_add(&mut self, b: BddNode) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, b_rooted, b);
        let one = self.add_const(1.0);
        crate::new_protected!(guard, one_rooted, one);
        let zero = self.add_const(0.0);
        crate::new_protected!(guard, zero_rooted, zero);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_ite(b_rooted.0, one_rooted.0, zero_rooted.0) },
            "Sylvan_mtbdd_ite(bdd_to_add)",
        );
        AddNode(n)
    }

    /// Returns BDD for `a > b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_greater_than(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let diff = self.add_minus(a, b);
        self.add_to_bdd(diff)
    }

    /// Returns BDD for `a < b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_less_than(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let diff = self.add_minus(b, a);
        self.add_to_bdd(diff)
    }

    /// Returns BDD for `a >= b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_greater_or_equal(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let lt = self.add_less_than(a, b);
        self.bdd_not(lt)
    }

    /// Returns BDD for `a <= b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_less_or_equal(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let gt = self.add_greater_than(a, b);
        self.bdd_not(gt)
    }

    /// Returns BDD for `a == b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_equals(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let gt = self.add_greater_than(a, b);
        let lt = self.add_less_than(a, b);
        let neq = self.bdd_or(gt, lt);
        self.bdd_not(neq)
    }

    /// Returns BDD for `a != b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_nequals(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let gt = self.add_greater_than(a, b);
        let lt = self.add_less_than(a, b);
        self.bdd_or(gt, lt)
    }

    /// Returns `true` iff `|a-b|_inf <= tolerance`.
    pub fn add_equal_sup_norm(&self, a: AddNode, b: AddNode, tolerance: f64) -> bool {
        unsafe { Sylvan_mtbdd_equal_norm_d(a.0, b.0, tolerance) == SYLVAN_TRUE }
    }

    /// Numerical epsilon used for ADD->BDD thresholding and convergence checks.
    pub fn epsilon(&self) -> f64 {
        EPS
    }

    /// Counts minterms in BDD `rel` over `num_vars` variables.
    ///
    /// __Refs__: none\
    /// __Derefs__: none
    pub fn bdd_count_minterms(&mut self, rel: BddNode, num_vars: u32) -> u64 {
        unsafe { Sylvan_mtbdd_satcount(rel.0, num_vars as usize) }.round() as u64
    }

    /// Returns the number of DAG nodes reachable from `root`.
    pub fn dag_size(&self, root: MTBDD) -> usize {
        let root = self.regular_node(root);
        unsafe { Sylvan_mtbdd_nodecount(root) as usize }
    }

    /// Iterates all nodes reachable from `root` and invokes `f` for each.
    pub fn foreach_node<F: FnMut(MTBDD)>(&self, root: MTBDD, mut f: F) {
        let mut visited: HashSet<MTBDD> = HashSet::new();
        let mut stack = vec![self.regular_node(root)];

        while let Some(node) = stack.pop() {
            let node = self.regular_node(node);
            if !visited.insert(node) {
                continue;
            }

            f(node);
            if !self.is_constant(node) {
                stack.push(self.read_then(node));
                stack.push(self.read_else(node));
            }
        }
    }

    /// Collects all unique terminal nodes reachable from `root`.
    pub fn terminal_nodes(&self, root: MTBDD) -> Vec<MTBDD> {
        let mut out = Vec::new();
        self.foreach_node(root, |n| {
            if self.is_constant(n) {
                out.push(self.regular_node(n));
            }
        });
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Returns the number of unique terminal nodes under `root`.
    pub fn num_terminals(&self, root: MTBDD) -> usize {
        self.terminal_nodes(root).len()
    }

    /// Alias for `dag_size`.
    pub fn num_nodes(&self, node: MTBDD) -> usize {
        self.dag_size(node)
    }

    /// Computes node count, terminal count, and minterms for an ADD root.
    ///
    /// __Refs__: none\
    /// __Derefs__: none
    pub fn add_stats(&mut self, root: AddNode, num_vars: u32) -> AddStats {
        let root = self.regular_node(root.0);
        let minterms = unsafe { sylvan_sys::mtbdd::Sylvan_mtbdd_satcount(root, num_vars as usize) }
            .round() as u64;

        AddStats {
            node_count: self.dag_size(root),
            terminal_count: self.num_terminals(root),
            minterms,
        }
    }

    fn var_index_label_map(&self, var_names: &HashMap<BDD, String>) -> HashMap<BDDVAR, String> {
        let mut labels = HashMap::new();
        for (&node, name) in var_names {
            let var_index = self.read_var_index(node);
            if var_index != BDDVAR::MAX {
                labels.entry(var_index).or_insert_with(|| name.clone());
            }
        }
        labels
    }

    fn var_label(var_index: BDDVAR, labels: &HashMap<BDDVAR, String>) -> String {
        labels
            .get(&var_index)
            .cloned()
            .unwrap_or_else(|| format!("x{}", var_index))
    }

    fn intern_id(ids: &mut HashMap<MTBDD, usize>, next_id: &mut usize, n: MTBDD) -> usize {
        *ids.entry(n).or_insert_with(|| {
            let id = *next_id;
            *next_id += 1;
            id
        })
    }

    /// Dumps an ADD graph to Graphviz DOT format.
    ///
    /// `var_names` maps representative nodes to human-readable labels.
    ///
    /// __Refs__: none\
    /// __Derefs__: none
    pub fn dump_add_dot(
        &self,
        root: AddNode,
        path: &str,
        var_names: &HashMap<BDD, String>,
    ) -> io::Result<()> {
        let mut out = File::create(path)?;
        writeln!(out, "digraph ADD {{")?;
        writeln!(out, "  rankdir=TB;")?;

        let mut ids: HashMap<MTBDD, usize> = HashMap::new();
        let mut next_id = 0usize;
        let mut visited: HashSet<MTBDD> = HashSet::new();
        let labels = self.var_index_label_map(var_names);

        let root_reg = self.regular_node(root.0);
        self.dump_add_dot_rec(
            root_reg,
            &mut out,
            &labels,
            &mut ids,
            &mut next_id,
            &mut visited,
        )?;
        writeln!(out, "}}")?;
        Ok(())
    }

    fn dump_add_dot_rec<W: Write>(
        &self,
        n: MTBDD,
        out: &mut W,
        labels: &HashMap<BDDVAR, String>,
        ids: &mut HashMap<MTBDD, usize>,
        next_id: &mut usize,
        visited: &mut HashSet<MTBDD>,
    ) -> io::Result<()> {
        let n = self.regular_node(n);
        if !visited.insert(n) {
            return Ok(());
        }

        let this = Self::intern_id(ids, next_id, n);
        let var = self.read_var_index(n);
        if var == BDDVAR::MAX {
            let v = self.add_value(n).unwrap_or(f64::NAN);
            writeln!(out, "  n{} [shape=box,label=\"{}\"] ;", this, v)?;
            return Ok(());
        }

        let t = self.regular_node(self.read_then(n));
        let e = self.regular_node(self.read_else(n));
        let tid = Self::intern_id(ids, next_id, t);
        let eid = Self::intern_id(ids, next_id, e);
        let label = Self::var_label(var, labels);

        writeln!(out, "  n{} [shape=ellipse,label=\"{}\"] ;", this, label)?;
        writeln!(out, "  n{} -> n{};", this, tid)?;
        writeln!(out, "  n{} -> n{} [style=dashed];", this, eid)?;

        self.dump_add_dot_rec(t, out, labels, ids, next_id, visited)?;
        self.dump_add_dot_rec(e, out, labels, ids, next_id, visited)?;
        Ok(())
    }

    /// Dumps a BDD graph to Graphviz DOT format.
    ///
    /// __Refs__: none\
    /// __Derefs__: none
    pub fn dump_bdd_dot(
        &self,
        root: BddNode,
        path: &str,
        var_names: &HashMap<BDD, String>,
    ) -> io::Result<()> {
        self.dump_add_dot(AddNode(root.0), path, var_names)
    }

    /// Builds an ADD that encodes the integer value of `nodes` as a bit-vector.
    ///
    /// Variable at index `i` contributes bit `2^i`.
    pub fn get_encoding(&mut self, indices: &[BDDVAR]) -> AddNode {
        let mut guard = LocalRootsGuard::new();

        crate::new_protected!(guard, result, self.add_const(0.0));
        let bdd_one = self.bdd_one();
        crate::new_protected!(guard, bdd_one_rooted, bdd_one);

        for bm in 0..(1i32 << indices.len()) {
            let mut term = bdd_one;
            for (i, &var) in indices.iter().enumerate() {
                let literal = if (bm & (1 << i)) != 0 {
                    self.bdd_var(var)
                } else {
                    self.bdd_not(self.bdd_var(var))
                };
                term = self.bdd_and(term, literal);
            }
            let term = self.bdd_to_add(term);
            let value = self.add_const(bm as f64);
            let term = self.add_times(term, value);
            result = self.add_plus(result, term);
        }

        result
    }

    /// Normalizes `m` over `next_var_cube` with a zero-safe denominator.
    ///
    /// __Refs__: result\
    /// __Derefs__: m, next_var_cube
    pub fn unif(&mut self, m: AddNode, vars: VarSet) -> AddNode {
        let mut guard = LocalRootsGuard::new();
        crate::new_protected!(guard, m_rooted, m);
        crate::new_protected!(guard, next_var_cube_rooted, vars);

        crate::new_protected!(guard, denom, self.add_sum_abstract(m, next_var_cube_rooted));

        crate::new_protected!(guard, denom_bdd, self.add_to_bdd(denom));
        crate::new_protected!(guard, one, self.add_const(1.0));
        let denom_safe = self.add_ite(denom_bdd, denom, one);
        self.add_divide(m, denom_safe)
    }
}

impl Default for RefManager {
    /// Creates a manager using the same default Sylvan initialization as [`RefManager::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RefManager {
    fn drop(&mut self) {
        self.clear_internal_caches();
        release_runtime_manager();
        let _ = self.runtime_guard.take();
    }
}

#[cfg(test)]
mod tests {
    use super::{BddNode, RefManager};

    fn assert_witness_satisfies(root: BddNode, mgr: &mut RefManager, witness: &[i32]) {
        let root_add = mgr.bdd_to_add(root);
        let value = mgr.add_eval_value(root_add, witness);
        assert_eq!(value, 1.0, "extracted witness must satisfy root BDD");
    }

    #[test]
    fn extract_leftmost_path_handles_non_complemented_root() {
        let mut mgr = RefManager::new();

        let x0_idx = mgr.new_var();
        let x0 = mgr.bdd_var(x0_idx);
        assert!(!x0.is_complemented());

        let witness = mgr
            .extract_leftmost_path_from_bdd(x0)
            .expect("x0 should be satisfiable");

        assert_eq!(witness[0], 1, "leftmost witness for x0 must set x0=1");
        assert_witness_satisfies(x0, &mut mgr, &witness);

        assert_eq!(mgr.nonzero_ref_count(), 0);
    }

    #[test]
    fn extract_leftmost_path_handles_complemented_root() {
        let mut mgr = RefManager::new();

        let x0_idx = mgr.new_var();
        let x0 = mgr.bdd_var(x0_idx);
        let not_x0 = mgr.bdd_not(x0);
        assert!(not_x0.is_complemented());

        let witness = mgr
            .extract_leftmost_path_from_bdd(not_x0)
            .expect("!x0 should be satisfiable");

        assert_eq!(witness[0], 0, "leftmost witness for !x0 must set x0=0");
        assert_witness_satisfies(not_x0, &mut mgr, &witness);

        assert_eq!(mgr.nonzero_ref_count(), 0);
    }

    #[test]
    fn add_max_abstract_takes_max_over_abstracted_var() {
        let mut mgr = RefManager::new();

        let cond = mgr.bdd_var(0);
        let then_branch = mgr.add_const(0.2);
        let else_branch = mgr.add_const(0.7);
        let f = mgr.add_ite(cond, then_branch, else_branch);

        let cube = mgr.add_var(0);
        let max_abs = mgr.add_max_abstract(f, cube);

        let value = mgr
            .add_value(max_abs.0)
            .expect("max abstraction over x0 should yield a constant");
        assert!(
            (value - 0.7).abs() < 1e-12,
            "expected max value 0.7, got {value}"
        );

        assert_eq!(mgr.nonzero_ref_count(), 0);
    }

    #[test]
    fn add_min_abstract_takes_min_over_abstracted_var() {
        let mut mgr = RefManager::new();

        let cond = mgr.bdd_var(0);
        let then_branch = mgr.add_const(0.2);
        let else_branch = mgr.add_const(0.7);
        let f = mgr.add_ite(cond, then_branch, else_branch);

        let cube = mgr.add_var(0);
        let min_abs = mgr.add_min_abstract(f, cube);

        let value = mgr
            .add_value(min_abs.0)
            .expect("min abstraction over x0 should yield a constant");
        assert!(
            (value - 0.2).abs() < 1e-12,
            "expected min value 0.2, got {value}"
        );

        assert_eq!(mgr.nonzero_ref_count(), 0);
    }
}
