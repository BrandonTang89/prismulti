//! Safe-ish, reference-count-aware wrapper around raw Sylvan operations.
//!
//! This module is the single place in the crate that should invoke Sylvan APIs
//! directly. It wraps raw MTBDD handles in lightweight node newtypes and
//! centralizes `Sylvan_ref`/`Sylvan_deref` bookkeeping so call sites can work
//! with higher-level BDD/ADD operations.

use std::{
    collections::{HashMap, HashSet},
    env,
    fs::File,
    io::{self, Write},
    sync::{Mutex, MutexGuard, OnceLock},
};

use sylvan_sys::{
    MTBDD, SYLVAN_FALSE, SYLVAN_INVALID, SYLVAN_TRUE,
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
        Sylvan_mtbdd_compose, Sylvan_mtbdd_deref, Sylvan_mtbdd_double, Sylvan_mtbdd_equal_norm_d,
        Sylvan_mtbdd_getdouble, Sylvan_mtbdd_hascomp, Sylvan_mtbdd_isleaf, Sylvan_mtbdd_ite,
        Sylvan_mtbdd_ithvar, Sylvan_mtbdd_minus, Sylvan_mtbdd_nodecount, Sylvan_mtbdd_plus,
        Sylvan_mtbdd_ref, Sylvan_mtbdd_satcount, Sylvan_mtbdd_strict_threshold_double,
        Sylvan_mtbdd_times, Sylvan_set_fromarray, Sylvan_var,
    },
};

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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
/// Opaque raw identity for Sylvan DD nodes.
pub struct Node(MTBDD);

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "N{:x}", self.0)
    }
}

impl Node {
    #[inline]
    fn raw(self) -> MTBDD {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Typed wrapper for BDD nodes.
pub struct BddNode(pub Node);

impl BddNode {
    #[inline]
    /// Returns the regular (non-complemented) view of this node.
    pub fn regular(self) -> Self {
        Self(Node(regular_raw(self.0.raw())))
    }

    #[inline]
    /// Returns `true` if this node is complement-tagged.
    pub fn is_complemented(self) -> bool {
        is_complemented_raw(self.0.raw())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Typed wrapper for ADD nodes.
pub struct AddNode(pub Node);

#[derive(Default)]
struct SylvanRuntime {
    initialized: bool,
}

/// Owns a Sylvan runtime handle and provides typed BDD/ADD operations.
pub struct RefManager {
    next_var_index: usize,
    owned_refs: HashMap<Node, usize>,
    internal_cache_ref_counts: HashMap<Node, usize>,
    cube_set_cache: HashMap<Node, Node>,
    var_set_cache: HashMap<Vec<u16>, Node>,
    swap_map_cache: HashMap<(Vec<u16>, Vec<u16>), Node>,
    track_owned_refs: bool,
    runtime_guard: Option<MutexGuard<'static, ()>>,
}

fn env_u32(name: &str) -> Option<u32> {
    env::var(name).ok().and_then(|raw| raw.parse::<u32>().ok())
}

fn env_bool(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        Err(_) => default,
    }
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
            owned_refs: HashMap::new(),
            internal_cache_ref_counts: HashMap::new(),
            cube_set_cache: HashMap::new(),
            var_set_cache: HashMap::new(),
            swap_map_cache: HashMap::new(),
            track_owned_refs: env_bool("PRISM_TRACK_REFS", cfg!(debug_assertions)),
            runtime_guard: Some(runtime_guard),
        }
    }

    /// Returns Sylvan's shared BDD one node without changing references.
    fn one_bdd(&self) -> BddNode {
        BddNode(Node(SYLVAN_TRUE))
    }

    fn zero_bdd(&self) -> BddNode {
        BddNode(Node(SYLVAN_FALSE))
    }

    fn must_node(&self, n: MTBDD, op: &str) -> Node {
        assert!(n != SYLVAN_INVALID, "Sylvan returned INVALID in {op}");
        Node(n)
    }

    #[inline]
    fn regular_node(&self, node: Node) -> Node {
        Node(regular_raw(node.raw()))
    }

    fn ensure_var_index(&mut self, idx: u16) {
        self.next_var_index = self.next_var_index.max(idx as usize + 1);
    }

    fn cube_to_set(&self, cube: Node) -> Node {
        let mut vars = HashSet::new();
        let mut visited = HashSet::new();
        self.collect_support_vars(cube, &mut visited, &mut vars);

        let mut indices: Vec<u16> = vars.into_iter().collect();
        indices.sort_unstable();
        self.var_set_from_indices(&indices)
    }

    fn collect_support_vars(
        &self,
        node: Node,
        visited: &mut HashSet<Node>,
        vars: &mut HashSet<u16>,
    ) {
        let reg = self.regular_node(node);
        if !visited.insert(reg) || self.is_constant(reg) {
            return;
        }

        vars.insert(self.read_var_index(reg));
        self.collect_support_vars(self.read_then(reg), visited, vars);
        self.collect_support_vars(self.read_else(reg), visited, vars);
    }

    fn var_set_from_indices(&self, vars: &[u16]) -> Node {
        let mut arr: Vec<u32> = vars.iter().map(|&v| u32::from(v)).collect();
        self.must_node(
            unsafe { Sylvan_set_fromarray(arr.as_mut_ptr(), arr.len()) },
            "Sylvan_set_fromarray",
        )
    }

    fn get_or_build_var_set_from_indices(&mut self, vars: &[u16]) -> Node {
        let key = vars.to_vec();
        if let Some(&set) = self.var_set_cache.get(&key) {
            return set;
        }

        let set = self.var_set_from_indices(vars);
        self.ref_node(set);
        self.mark_internal_cache_ref(set);
        self.var_set_cache.insert(key, set);
        set
    }

    fn get_or_build_cube_set(&mut self, cube: Node) -> Node {
        let key = self.regular_node(cube);
        if let Some(&vars) = self.cube_set_cache.get(&key) {
            return vars;
        }

        let vars = self.cube_to_set(key);
        self.ref_node(vars);
        self.mark_internal_cache_ref(vars);
        self.cube_set_cache.insert(key, vars);
        vars
    }

    fn build_swap_map_uncached(&mut self, x: &[u16], y: &[u16]) -> Node {
        let mut map = self.must_node(unsafe { Sylvan_map_empty() }, "Sylvan_map_empty");
        self.ref_node(map);

        for (&xi, &yi) in x.iter().zip(y.iter()) {
            self.ensure_var_index(xi);
            self.ensure_var_index(yi);

            let y_var = self.must_node(unsafe { Sylvan_ithvar(u32::from(yi)) }, "Sylvan_ithvar(y)");
            self.ref_node(y_var);
            let new_map_xy = self.must_node(
                unsafe { Sylvan_map_add(map.raw(), u32::from(xi), y_var.raw()) },
                "Sylvan_map_add(x->y)",
            );
            self.ref_node(new_map_xy);
            self.deref_node(map);
            self.deref_node(y_var);
            map = new_map_xy;

            let x_var = self.must_node(unsafe { Sylvan_ithvar(u32::from(xi)) }, "Sylvan_ithvar(x)");
            self.ref_node(x_var);
            let new_map_yx = self.must_node(
                unsafe { Sylvan_map_add(map.raw(), u32::from(yi), x_var.raw()) },
                "Sylvan_map_add(y->x)",
            );
            self.ref_node(new_map_yx);
            self.deref_node(map);
            self.deref_node(x_var);
            map = new_map_yx;
        }

        map
    }

    fn get_or_build_swap_map(&mut self, x: &[u16], y: &[u16]) -> Node {
        let key = (x.to_vec(), y.to_vec());
        if let Some(&map) = self.swap_map_cache.get(&key) {
            return map;
        }

        let map = self.build_swap_map_uncached(x, y);
        self.mark_internal_cache_ref(map);
        self.swap_map_cache.insert(key, map);
        map
    }

    fn mark_internal_cache_ref(&mut self, node: Node) {
        let reg = self.regular_node(node);
        *self.internal_cache_ref_counts.entry(reg).or_insert(0) += 1;
    }

    fn unmark_internal_cache_ref(&mut self, node: Node) {
        let reg = self.regular_node(node);
        if let Some(count) = self.internal_cache_ref_counts.get_mut(&reg) {
            *count -= 1;
            if *count == 0 {
                self.internal_cache_ref_counts.remove(&reg);
            }
        } else {
            debug_assert!(false, "internal cache deref of untracked node: {:?}", reg);
        }
    }

    fn track_ref(&mut self, node: Node) {
        let reg = self.regular_node(node);
        unsafe {
            Sylvan_mtbdd_ref(reg.raw());
        }
        if self.track_owned_refs {
            *self.owned_refs.entry(reg).or_insert(0) += 1;
        }
    }

    fn track_deref(&mut self, node: Node) {
        let reg = self.regular_node(node);
        unsafe {
            Sylvan_mtbdd_deref(reg.raw());
        }
        if self.track_owned_refs {
            if let Some(count) = self.owned_refs.get_mut(&reg) {
                *count -= 1;
                if *count == 0 {
                    self.owned_refs.remove(&reg);
                }
            } else {
                debug_assert!(false, "deref of untracked node: {:?}", reg);
            }
        }
    }

    /// Releases internal memoization roots that are retained by the manager.
    pub fn clear_internal_caches(&mut self) {
        let cube_sets: Vec<Node> = self.cube_set_cache.drain().map(|(_, n)| n).collect();
        for n in cube_sets {
            self.unmark_internal_cache_ref(n);
            self.deref_node(n);
        }

        let var_sets: Vec<Node> = self.var_set_cache.drain().map(|(_, n)| n).collect();
        for n in var_sets {
            self.unmark_internal_cache_ref(n);
            self.deref_node(n);
        }

        let swap_maps: Vec<Node> = self.swap_map_cache.drain().map(|(_, n)| n).collect();
        for n in swap_maps {
            self.unmark_internal_cache_ref(n);
            self.deref_node(n);
        }
    }

    /// Increments the Sylvan reference count of `node` and returns it.
    pub fn ref_node(&mut self, node: Node) -> Node {
        self.track_ref(node);
        node
    }

    /// Decrements the Sylvan reference count of `node` and returns it.
    pub fn deref_node(&mut self, node: Node) -> Node {
        self.track_deref(node);
        node
    }

    /// Returns the number of nodes still carrying non-zero references.
    pub fn nonzero_ref_count(&self) -> usize {
        if !self.track_owned_refs {
            return 0;
        }

        self.owned_refs
            .iter()
            .filter(|(node, count)| {
                let internal = self
                    .internal_cache_ref_counts
                    .get(*node)
                    .copied()
                    .unwrap_or(0);
                **count > internal
            })
            .count()
    }

    /// Returns the number of DD variables currently allocated in this manager view.
    pub fn var_count(&self) -> usize {
        self.next_var_index
    }

    /// Validate tracked node validity.
    pub fn debug_check(&self) -> bool {
        self.owned_refs
            .keys()
            .all(|n| unsafe { sylvan_sys::mtbdd::Sylvan_mtbdd_test_isvalid(n.raw()) != 0 })
    }

    /// Reads the variable index for `node`, or `u16::MAX` for constants.
    pub fn read_var_index(&self, node: Node) -> u16 {
        if self.is_constant(node) {
            u16::MAX
        } else {
            let reg = self.regular_node(node);
            unsafe { Sylvan_var(reg.raw()) as u16 }
        }
    }

    /// Returns the THEN child of a node, preserving complement semantics.
    pub fn read_then(&self, node: Node) -> Node {
        if self.is_constant(node) {
            self.regular_node(node)
        } else {
            self.must_node(unsafe { Sylvan_high(node.raw()) }, "Sylvan_high")
        }
    }

    /// Returns the ELSE child of a node, preserving complement semantics.
    pub fn read_else(&self, node: Node) -> Node {
        if self.is_constant(node) {
            self.regular_node(node)
        } else {
            self.must_node(unsafe { Sylvan_low(node.raw()) }, "Sylvan_low")
        }
    }

    /// Returns `true` if `node` is a terminal (constant) node.
    pub fn is_constant(&self, node: Node) -> bool {
        let reg = self.regular_node(node);
        unsafe { Sylvan_mtbdd_isleaf(reg.raw()) != 0 }
    }

    /// Returns the terminal value for constant nodes, otherwise `None`.
    pub fn add_value(&self, node: Node) -> Option<f64> {
        if !self.is_constant(node) {
            return None;
        }

        if node.raw() == SYLVAN_FALSE {
            return Some(0.0);
        }
        if node.raw() == SYLVAN_TRUE {
            return Some(1.0);
        }

        let reg = self.regular_node(node);
        let v = leaf_to_f64(reg.raw());
        if is_complemented_raw(node.raw()) {
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
        BddNode(self.ref_node(self.one_bdd().0))
    }

    /// Returns a newly referenced BDD zero node.
    pub fn bdd_zero(&mut self) -> BddNode {
        BddNode(self.ref_node(self.zero_bdd().0))
    }

    /// Returns a newly referenced ADD constant zero node.
    pub fn add_zero(&mut self) -> AddNode {
        self.add_const(0.0)
    }

    /// Returns a newly referenced ADD constant node.
    pub fn add_const(&mut self, value: f64) -> AddNode {
        let n = self.must_node(unsafe { Sylvan_mtbdd_double(value) }, "Sylvan_mtbdd_double");
        AddNode(self.ref_node(n))
    }

    /// Allocates a new BDD variable and returns it referenced.
    pub fn new_var(&mut self) -> BddNode {
        let idx = self.next_var_index as u16;
        self.next_var_index += 1;
        self.bdd_var(idx)
    }

    /// Returns the BDD variable node for `var_index`, referenced.
    pub fn bdd_var(&mut self, var_index: u16) -> BddNode {
        self.ensure_var_index(var_index);
        let n = self.must_node(
            unsafe { Sylvan_ithvar(u32::from(var_index)) },
            "Sylvan_ithvar",
        );
        BddNode(self.ref_node(n))
    }

    /// Returns the ADD variable node for `var_index`, referenced.
    pub fn add_var(&mut self, var_index: u16) -> AddNode {
        self.ensure_var_index(var_index);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_ithvar(u32::from(var_index)) },
            "Sylvan_mtbdd_ithvar",
        );
        AddNode(self.ref_node(n))
    }

    /// Logical negation of `a`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a
    pub fn bdd_not(&mut self, a: BddNode) -> BddNode {
        let n = self.must_node(unsafe { Sylvan_not(a.0.raw()) }, "Sylvan_not");
        self.ref_node(n);
        self.deref_node(a.0);
        BddNode(n)
    }

    /// Boolean equivalence (`XNOR`) of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn bdd_equals(&mut self, a: BddNode, b: BddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Sylvan_equiv(a.0.raw(), b.0.raw()) },
            "Sylvan_equiv",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        BddNode(n)
    }

    /// Boolean inequality (`XOR`) of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn bdd_nequals(&mut self, a: BddNode, b: BddNode) -> BddNode {
        let n = self.must_node(unsafe { Sylvan_xor(a.0.raw(), b.0.raw()) }, "Sylvan_xor");
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        BddNode(n)
    }

    /// Conjunction of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn bdd_and(&mut self, a: BddNode, b: BddNode) -> BddNode {
        let n = self.must_node(unsafe { Sylvan_and(a.0.raw(), b.0.raw()) }, "Sylvan_and");
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        BddNode(n)
    }

    /// Disjunction of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn bdd_or(&mut self, a: BddNode, b: BddNode) -> BddNode {
        let n = self.must_node(unsafe { Sylvan_or(a.0.raw(), b.0.raw()) }, "Sylvan_or");
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        BddNode(n)
    }

    /// Existentially abstracts variables in `cube` from `a`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a
    pub fn bdd_exists_abstract(&mut self, a: BddNode, cube: BddNode) -> BddNode {
        let vars = self.get_or_build_cube_set(cube.0);

        let n = self.must_node(
            unsafe { Sylvan_exists(a.0.raw(), vars.raw()) },
            "Sylvan_exists",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        BddNode(n)
    }

    /// Computes `(f AND g)` then existentially abstracts variables in `cube`.\
    /// This is essentially matrix multiplication for BDDs\
    /// __Refs__: result\
    /// __Derefs__: f, g
    pub fn bdd_and_then_existsabs(&mut self, f: BddNode, g: BddNode, cube: BddNode) -> BddNode {
        let vars = self.get_or_build_cube_set(cube.0);

        let n = self.must_node(
            unsafe { Sylvan_and_exists(f.0.raw(), g.0.raw(), vars.raw()) },
            "Sylvan_and_exists",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        self.deref_node(g.0);
        BddNode(n)
    }

    /// Swaps each variable in `x` with the corresponding variable in `y`.
    ///
    /// both index slices must have the same length.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn bdd_swap_variables(&mut self, f: BddNode, x: &[u16], y: &[u16]) -> BddNode {
        assert_eq!(x.len(), y.len());
        let map = self.get_or_build_swap_map(x, y);
        let n = self.must_node(
            unsafe { Sylvan_compose(f.0.raw(), map.raw()) },
            "Sylvan_compose",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        BddNode(n)
    }

    /// Swaps each variable in `x` with the corresponding variable in `y` for ADD `f`.
    ///
    /// Both index slices must have the same length.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_swap_vars(&mut self, f: AddNode, x: &[u16], y: &[u16]) -> AddNode {
        assert_eq!(x.len(), y.len());
        let map = self.get_or_build_swap_map(x, y);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_compose(f.0.raw(), map.raw()) },
            "Sylvan_mtbdd_compose",
        );
        self.ref_node(n);
        self.deref_node(f.0);
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
    pub fn add_matrix_multiply(&mut self, a: AddNode, b: AddNode, z: &[u16]) -> AddNode {
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
        vars: Node,
    ) -> AddNode {
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_and_abstract_plus(a.0.raw(), b.0.raw(), vars.raw()) },
            "Sylvan_mtbdd_and_abstract_plus",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// Returns an internal cached variable-set node for `vars`.
    pub fn get_var_set_for_indices(&mut self, vars: &[u16]) -> Node {
        self.get_or_build_var_set_from_indices(vars)
    }

    /// Returns an internal cached swap-map node for `(x, y)`.
    pub fn get_swap_map_for_indices(&mut self, x: &[u16], y: &[u16]) -> Node {
        self.get_or_build_swap_map(x, y)
    }

    /// Composes BDD `f` with precomputed map `map`.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn bdd_compose_with_map(&mut self, f: BddNode, map: Node) -> BddNode {
        let n = self.must_node(
            unsafe { Sylvan_compose(f.0.raw(), map.raw()) },
            "Sylvan_compose",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        BddNode(n)
    }

    /// Composes ADD `f` with precomputed map `map`.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_compose_with_map(&mut self, f: AddNode, map: Node) -> AddNode {
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_compose(f.0.raw(), map.raw()) },
            "Sylvan_mtbdd_compose",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        AddNode(n)
    }

    /// Pointwise ADD addition of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_plus(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_plus(a.0.raw(), b.0.raw()) },
            "Sylvan_mtbdd_plus",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// Pointwise ADD subtraction `a - b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_minus(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_minus(a.0.raw(), b.0.raw()) },
            "Sylvan_mtbdd_minus",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// Pointwise ADD multiplication of `a` and `b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_times(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_times(a.0.raw(), b.0.raw()) },
            "Sylvan_mtbdd_times",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// Pointwise ADD division `a / b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_divide(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let op: MTBDD_APPLY_OP = mtbdd_divide_op;
        let n = self.must_node(
            unsafe { sylvan_sys::mtbdd::Sylvan_mtbdd_apply(a.0.raw(), b.0.raw(), op) },
            "Sylvan_mtbdd_apply(divide)",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
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
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_ite(cond.0.raw(), then_branch.0.raw(), else_branch.0.raw()) },
            "Sylvan_mtbdd_ite",
        );
        self.ref_node(n);
        self.deref_node(cond.0);
        self.deref_node(then_branch.0);
        self.deref_node(else_branch.0);
        AddNode(n)
    }

    /// Existential abstraction over ADD `f` with respect to `cube`.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_sum_abstract(&mut self, f: AddNode, cube: AddNode) -> AddNode {
        let vars = self.get_or_build_cube_set(cube.0);

        let n = self.must_node(
            unsafe { Sylvan_mtbdd_abstract_plus(f.0.raw(), vars.raw()) },
            "Sylvan_mtbdd_abstract_plus",
        );
        self.ref_node(n);
        self.deref_node(f.0);
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
        let vars = self.get_or_build_cube_set(cube.0);

        let n = self.must_node(
            unsafe { Sylvan_mtbdd_abstract_max(f.0.raw(), vars.raw()) },
            "Sylvan_mtbdd_abstract_max",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        AddNode(n)
    }

    /// Min abstraction over ADD `f` with respect to `cube`.
    ///
    /// __Refs__: result\
    /// __Derefs__: f
    pub fn add_min_abstract(&mut self, f: AddNode, cube: AddNode) -> AddNode {
        let vars = self.get_or_build_cube_set(cube.0);

        let n = self.must_node(
            unsafe { Sylvan_mtbdd_abstract_min(f.0.raw(), vars.raw()) },
            "Sylvan_mtbdd_abstract_min",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        AddNode(n)
    }

    /// Converts an ADD to a BDD using threshold `EPS`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a
    pub fn add_to_bdd(&mut self, a: AddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_strict_threshold_double(a.0.raw(), EPS) },
            "Sylvan_mtbdd_strict_threshold_double",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        BddNode(n)
    }

    /// Converts an ADD to its support-pattern BDD.
    ///
    /// __Refs__: result\
    /// __Derefs__: a
    pub fn add_to_bdd_pattern(&mut self, a: AddNode) -> BddNode {
        self.ref_node(a.0);
        let zero_for_gt = self.add_const(0.0);
        let gt_zero = self.add_greater_than(a, zero_for_gt);

        let zero_for_lt = self.add_const(0.0);
        let lt_zero = self.add_less_than(AddNode(a.0), zero_for_lt);
        self.bdd_or(gt_zero, lt_zero)
    }

    /// Converts a BDD to an ADD.
    ///
    /// __Refs__: result\
    /// __Derefs__: b
    pub fn bdd_to_add(&mut self, b: BddNode) -> AddNode {
        let one = self.add_const(1.0);
        let zero = self.add_const(0.0);
        let n = self.must_node(
            unsafe { Sylvan_mtbdd_ite(b.0.raw(), one.0.raw(), zero.0.raw()) },
            "Sylvan_mtbdd_ite(bdd_to_add)",
        );
        self.ref_node(n);
        self.deref_node(one.0);
        self.deref_node(zero.0);
        self.deref_node(b.0);
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
        self.ref_node(a.0);
        self.ref_node(b.0);
        let gt = self.add_greater_than(a, b);
        let lt = self.add_less_than(AddNode(a.0), AddNode(b.0));
        let neq = self.bdd_or(gt, lt);
        self.bdd_not(neq)
    }

    /// Returns BDD for `a != b`.
    ///
    /// __Refs__: result\
    /// __Derefs__: a, b
    pub fn add_nequals(&mut self, a: AddNode, b: AddNode) -> BddNode {
        self.ref_node(a.0);
        self.ref_node(b.0);
        let gt = self.add_greater_than(a, b);
        let lt = self.add_less_than(AddNode(a.0), AddNode(b.0));
        self.bdd_or(gt, lt)
    }

    /// Returns `true` iff `|a-b|_inf <= tolerance`.
    pub fn add_equal_sup_norm(&self, a: AddNode, b: AddNode, tolerance: f64) -> bool {
        unsafe { Sylvan_mtbdd_equal_norm_d(a.0.raw(), b.0.raw(), tolerance) == SYLVAN_TRUE }
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
        unsafe { Sylvan_mtbdd_satcount(rel.0.raw(), num_vars as usize) }.round() as u64
    }

    /// Returns the number of DAG nodes reachable from `root`.
    pub fn dag_size(&self, root: Node) -> usize {
        let root = self.regular_node(root);
        unsafe { Sylvan_mtbdd_nodecount(root.raw()) as usize }
    }

    /// Iterates all nodes reachable from `root` and invokes `f` for each.
    pub fn foreach_node<F: FnMut(Node)>(&self, root: Node, mut f: F) {
        let mut visited: HashSet<Node> = HashSet::new();
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
    pub fn terminal_nodes(&self, root: Node) -> Vec<Node> {
        let mut out = Vec::new();
        self.foreach_node(root, |n| {
            if self.is_constant(n) {
                out.push(self.regular_node(n));
            }
        });
        out.sort_by_key(|n| n.0);
        out.dedup();
        out
    }

    /// Returns the number of unique terminal nodes under `root`.
    pub fn num_terminals(&self, root: Node) -> usize {
        self.terminal_nodes(root).len()
    }

    /// Alias for `dag_size`.
    pub fn num_nodes(&self, node: Node) -> usize {
        self.dag_size(node)
    }

    /// Computes node count, terminal count, and minterms for an ADD root.
    ///
    /// __Refs__: none\
    /// __Derefs__: none
    pub fn add_stats(&mut self, root: AddNode, num_vars: u32) -> AddStats {
        let root = self.regular_node(root.0);
        let minterms =
            unsafe { sylvan_sys::mtbdd::Sylvan_mtbdd_satcount(root.raw(), num_vars as usize) }
                .round() as u64;

        AddStats {
            node_count: self.dag_size(root),
            terminal_count: self.num_terminals(root),
            minterms,
        }
    }

    fn var_index_label_map(&self, var_names: &HashMap<Node, String>) -> HashMap<u16, String> {
        let mut labels = HashMap::new();
        for (&node, name) in var_names {
            let var_index = self.read_var_index(node);
            if var_index != u16::MAX {
                labels.entry(var_index).or_insert_with(|| name.clone());
            }
        }
        labels
    }

    fn var_label(var_index: u16, labels: &HashMap<u16, String>) -> String {
        labels
            .get(&var_index)
            .cloned()
            .unwrap_or_else(|| format!("x{}", var_index))
    }

    fn intern_id(ids: &mut HashMap<Node, usize>, next_id: &mut usize, n: Node) -> usize {
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
        var_names: &HashMap<Node, String>,
    ) -> io::Result<()> {
        let mut out = File::create(path)?;
        writeln!(out, "digraph ADD {{")?;
        writeln!(out, "  rankdir=TB;")?;

        let mut ids: HashMap<Node, usize> = HashMap::new();
        let mut next_id = 0usize;
        let mut visited: HashSet<Node> = HashSet::new();
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
        n: Node,
        out: &mut W,
        labels: &HashMap<u16, String>,
        ids: &mut HashMap<Node, usize>,
        next_id: &mut usize,
        visited: &mut HashSet<Node>,
    ) -> io::Result<()> {
        let n = self.regular_node(n);
        if !visited.insert(n) {
            return Ok(());
        }

        let this = Self::intern_id(ids, next_id, n);
        let var = self.read_var_index(n);
        if var == u16::MAX {
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
        var_names: &HashMap<Node, String>,
    ) -> io::Result<()> {
        self.dump_add_dot(AddNode(root.0), path, var_names)
    }

    /// Builds an ADD that encodes the integer value of `nodes` as a bit-vector.
    ///
    /// Variable at index `i` contributes bit `2^i`.
    pub fn get_encoding(&mut self, nodes: &[Node]) -> AddNode {
        let mut result = self.add_const(0.0);
        let bdd_one = self.bdd_one();

        for bm in 0..(1i32 << nodes.len()) {
            self.ref_node(bdd_one.0);
            let mut term = bdd_one;
            for (i, &var) in nodes.iter().enumerate() {
                self.ref_node(var);
                let literal = if (bm & (1 << i)) != 0 {
                    BddNode(var)
                } else {
                    self.bdd_not(BddNode(var))
                };
                term = self.bdd_and(term, literal);
            }
            let term = self.bdd_to_add(term);
            let value = self.add_const(bm as f64);
            let term = self.add_times(term, value);
            result = self.add_plus(result, term);
        }

        self.deref_node(bdd_one.0);
        result
    }

    /// Normalizes `m` over `next_var_cube` with a zero-safe denominator.
    ///
    /// __Refs__: result\
    /// __Derefs__: m, next_var_cube
    pub fn unif(&mut self, m: AddNode, next_var_cube: BddNode) -> AddNode {
        self.ref_node(m.0);
        self.ref_node(next_var_cube.0);
        let next_cube_add = self.bdd_to_add(next_var_cube);
        let denom = self.add_sum_abstract(m, next_cube_add);
        self.deref_node(next_cube_add.0);

        self.ref_node(denom.0);
        let denom_bdd = self.add_to_bdd(denom);
        let one = self.add_const(1.0);
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
        mgr.ref_node(root.0);
        let root_add = mgr.bdd_to_add(root);
        let value = mgr.add_eval_value(root_add, witness);
        assert_eq!(value, 1.0, "extracted witness must satisfy root BDD");
        mgr.deref_node(root_add.0);
    }

    #[test]
    fn extract_leftmost_path_handles_non_complemented_root() {
        let mut mgr = RefManager::new();

        let x0 = mgr.new_var();
        assert!(!x0.is_complemented());

        let witness = mgr
            .extract_leftmost_path_from_bdd(x0)
            .expect("x0 should be satisfiable");

        assert_eq!(witness[0], 1, "leftmost witness for x0 must set x0=1");
        assert_witness_satisfies(x0, &mut mgr, &witness);

        mgr.deref_node(x0.0);
        assert_eq!(mgr.nonzero_ref_count(), 0);
    }

    #[test]
    fn extract_leftmost_path_handles_complemented_root() {
        let mut mgr = RefManager::new();

        let x0 = mgr.new_var();
        let not_x0 = mgr.bdd_not(x0);
        assert!(not_x0.is_complemented());

        let witness = mgr
            .extract_leftmost_path_from_bdd(not_x0)
            .expect("!x0 should be satisfiable");

        assert_eq!(witness[0], 0, "leftmost witness for !x0 must set x0=0");
        assert_witness_satisfies(not_x0, &mut mgr, &witness);

        mgr.deref_node(not_x0.0);
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

        mgr.deref_node(max_abs.0);
        mgr.deref_node(cube.0);
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

        mgr.deref_node(min_abs.0);
        mgr.deref_node(cube.0);
        assert_eq!(mgr.nonzero_ref_count(), 0);
    }
}
