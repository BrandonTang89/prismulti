//! Safe-ish, reference-count-aware wrapper around raw CUDD operations.
//!
//! This module is the single place in the crate that should invoke CUDD APIs
//! directly. It wraps raw pointers in lightweight node newtypes and centralizes
//! `Cudd_Ref`/`Cudd_RecursiveDeref` bookkeeping so call sites can work with
//! higher-level BDD/ADD operations.
//!
//! ## Ownership model
//! - Most combinators consume their input handles logically and return a newly
//!   referenced result.
//! - Methods that internally keep an argument alive without consuming it are
//!   documented explicitly.
//! - `RefManager` owns the underlying `DdManager` and releases it in `Drop`.

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Write},
    os::raw::c_int,
    ptr,
};

use cudd_sys::{
    DdManager, DdNode,
    cudd::{
        CUDD_CACHE_SLOTS, CUDD_UNIQUE_SLOTS, Cudd_BddToAdd, Cudd_CheckZeroRef, Cudd_CountMinterm,
        Cudd_DagSize, Cudd_DebugCheck, Cudd_E, Cudd_EqualSupNorm, Cudd_Eval, Cudd_ForeachNode,
        Cudd_IsComplement, Cudd_IsConstant, Cudd_NodeReadIndex, Cudd_Not, Cudd_Quit,
        Cudd_ReadLogicZero, Cudd_ReadOne, Cudd_ReadSize, Cudd_RecursiveDeref, Cudd_Ref,
        Cudd_Regular, Cudd_T, Cudd_V, Cudd_addApply, Cudd_addBddPattern, Cudd_addBddThreshold,
        Cudd_addConst, Cudd_addDivide, Cudd_addExistAbstract, Cudd_addIte, Cudd_addIthVar,
        Cudd_addMatrixMultiply, Cudd_addMaxAbstract, Cudd_addMinAbstract, Cudd_addMinus,
        Cudd_addOrAbstract, Cudd_addPlus, Cudd_addSwapVariables, Cudd_addTimes, Cudd_bddAnd,
        Cudd_bddAndAbstract, Cudd_bddExistAbstract, Cudd_bddIthVar, Cudd_bddNewVar, Cudd_bddOr,
        Cudd_bddSwapVariables, Cudd_bddXnor, Cudd_bddXor, DD_APPLY_OPERATOR,
    },
};

pub const EPS: f64 = 1e-10;
/// Maximum number of leaked nodes reported by leak diagnostics.
pub static LEAK_REPORT_LIMIT: usize = 10;
const ENABLE_CUDD_DEBUGCHECK_ON_DROP: bool = true;

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
/// Opaque raw pointer identity for CUDD nodes.
pub struct Node(*mut DdNode);

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "N{:x}", self.0 as usize)
    }
}

impl Node {
    #[inline]
    fn as_ptr(self) -> *mut DdNode {
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
        Self(Node(unsafe { Cudd_Regular(self.0.as_ptr()) }))
    }

    #[inline]
    /// Returns `true` if this node is complement-tagged.
    pub fn is_complemented(self) -> bool {
        unsafe { Cudd_IsComplement(self.0.as_ptr()) != 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Typed wrapper for ADD nodes.
pub struct AddNode(pub Node);

/// Owns a CUDD manager and provides typed BDD/ADD operations.
pub struct RefManager {
    mgr: *mut DdManager,
}

extern "C" fn add_plus_op(
    dd: *mut DdManager,
    f: *mut *mut DdNode,
    g: *mut *mut DdNode,
) -> *mut DdNode {
    unsafe { Cudd_addPlus(dd, f, g) }
}

extern "C" fn add_minus_op(
    dd: *mut DdManager,
    f: *mut *mut DdNode,
    g: *mut *mut DdNode,
) -> *mut DdNode {
    unsafe { Cudd_addMinus(dd, f, g) }
}

extern "C" fn add_times_op(
    dd: *mut DdManager,
    f: *mut *mut DdNode,
    g: *mut *mut DdNode,
) -> *mut DdNode {
    unsafe { Cudd_addTimes(dd, f, g) }
}

extern "C" fn add_divide_op(
    dd: *mut DdManager,
    f: *mut *mut DdNode,
    g: *mut *mut DdNode,
) -> *mut DdNode {
    unsafe { Cudd_addDivide(dd, f, g) }
}

impl RefManager {
    /// Creates a new CUDD manager with default unique/cache sizing.
    pub fn new() -> Self {
        let mgr =
            unsafe { cudd_sys::cudd::Cudd_Init(0, 0, CUDD_UNIQUE_SLOTS, CUDD_CACHE_SLOTS, 0) };
        assert!(!mgr.is_null(), "Failed to initialize CUDD manager");
        Self { mgr }
    }

    /// Returns CUDD's shared BDD one node without changing references.
    fn one_bdd(&self) -> BddNode {
        BddNode(Node(unsafe { Cudd_ReadOne(self.mgr) }))
    }

    fn zero_bdd(&self) -> BddNode {
        BddNode(Node(unsafe { Cudd_ReadLogicZero(self.mgr) }))
    }

    fn must_node(&self, p: *mut DdNode, op: &str) -> Node {
        assert!(!p.is_null(), "CUDD returned NULL in {op}");
        Node(p)
    }

    #[inline]
    fn regular_node(&self, node: Node) -> Node {
        Node(unsafe { Cudd_Regular(node.as_ptr()) })
    }

    #[inline]
    fn is_complemented_node(&self, node: Node) -> bool {
        unsafe { Cudd_IsComplement(node.as_ptr()) != 0 }
    }

    fn add_apply(&self, op: DD_APPLY_OPERATOR, a: Node, b: Node, op_name: &str) -> Node {
        self.must_node(
            unsafe { Cudd_addApply(self.mgr, op, a.as_ptr(), b.as_ptr()) },
            op_name,
        )
    }

    fn track_ref(&mut self, node: Node) {
        let reg = self.regular_node(node);
        unsafe { Cudd_Ref(reg.as_ptr()) };
    }

    fn track_deref(&mut self, node: Node) {
        let reg = self.regular_node(node);
        unsafe { Cudd_RecursiveDeref(self.mgr, reg.as_ptr()) };
    }

    /// Increments the CUDD reference count of `node` and returns it.
    pub fn ref_node(&mut self, node: Node) -> Node {
        self.track_ref(node);
        node
    }

    /// Decrements the CUDD reference count of `node` and returns it.
    pub fn deref_node(&mut self, node: Node) -> Node {
        self.track_deref(node);
        node
    }

    /// Returns the number of nodes still carrying non-zero references.
    pub fn nonzero_ref_count(&self) -> usize {
        let raw = unsafe { Cudd_CheckZeroRef(self.mgr) };
        raw as usize
    }

    /// Returns the number of DD variables currently allocated in the manager.
    pub fn var_count(&self) -> usize {
        unsafe { Cudd_ReadSize(self.mgr) as usize }
    }

    /// Validate manager internal consistency.
    pub fn debug_check(&self) -> bool {
        unsafe { Cudd_DebugCheck(self.mgr) == 0 }
    }

    /// Reads the variable index for `node`, or `u16::MAX` for constants.
    pub fn read_var_index(&self, node: Node) -> u16 {
        let reg = self.regular_node(node);
        if self.is_constant(node) {
            u16::MAX
        } else {
            unsafe { Cudd_NodeReadIndex(reg.as_ptr()) as u16 }
        }
    }

    /// Returns the THEN child of a node, preserving complement semantics.
    pub fn read_then(&self, node: Node) -> Node {
        let reg = self.regular_node(node);
        if self.is_constant(node) {
            reg
        } else {
            let t = Node(unsafe { Cudd_T(reg.as_ptr()) });
            if self.is_complemented_node(node) {
                Node(unsafe { Cudd_Not(t.as_ptr()) })
            } else {
                t
            }
        }
    }

    /// Returns the ELSE child of a node, preserving complement semantics.
    pub fn read_else(&self, node: Node) -> Node {
        let reg = self.regular_node(node);
        if self.is_constant(node) {
            reg
        } else {
            let e = Node(unsafe { Cudd_E(reg.as_ptr()) });
            if self.is_complemented_node(node) {
                Node(unsafe { Cudd_Not(e.as_ptr()) })
            } else {
                e
            }
        }
    }

    /// Returns `true` if `node` is a terminal (constant) node.
    pub fn is_constant(&self, node: Node) -> bool {
        let reg = self.regular_node(node);
        unsafe { Cudd_IsConstant(reg.as_ptr()) != 0 }
    }

    /// Returns the terminal value for constant nodes, otherwise `None`.
    pub fn add_value(&self, node: Node) -> Option<f64> {
        let reg = self.regular_node(node);
        if self.is_constant(node) {
            let v = unsafe { Cudd_V(reg.as_ptr()) };
            if self.is_complemented_node(node) {
                Some(1.0 - v)
            } else {
                Some(v)
            }
        } else {
            None
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

        let mut eval_inputs: Vec<c_int> = inputs.iter().map(|&v| v as c_int).collect();
        let terminal = self.must_node(
            unsafe { Cudd_Eval(self.mgr, f.0.as_ptr(), eval_inputs.as_mut_ptr()) },
            "Cudd_Eval",
        );
        self.add_value(terminal)
            .expect("Cudd_Eval on ADD must return terminal")
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
        let n = self.must_node(unsafe { Cudd_addConst(self.mgr, value) }, "Cudd_addConst");
        AddNode(self.ref_node(n))
    }

    /// Allocates a new BDD variable and returns it referenced.
    pub fn new_var(&mut self) -> BddNode {
        let n = self.must_node(unsafe { Cudd_bddNewVar(self.mgr) }, "Cudd_bddNewVar");
        BddNode(self.ref_node(n))
    }

    /// Returns the BDD variable node for `var_index`, referenced.
    pub fn bdd_var(&mut self, var_index: u16) -> BddNode {
        let n = self.must_node(
            unsafe { Cudd_bddIthVar(self.mgr, var_index as i32) },
            "Cudd_bddIthVar",
        );
        BddNode(self.ref_node(n))
    }

    /// Returns the ADD variable node for `var_index`, referenced.
    pub fn add_var(&mut self, var_index: u16) -> AddNode {
        let n = self.must_node(
            unsafe { Cudd_addIthVar(self.mgr, var_index as i32) },
            "Cudd_addIthVar",
        );
        AddNode(self.ref_node(n))
    }

    /// Logical negation of `a`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a
    pub fn bdd_not(&mut self, a: BddNode) -> BddNode {
        let n = Node(unsafe { Cudd_Not(a.0.as_ptr()) });
        self.ref_node(n);
        self.deref_node(a.0);
        BddNode(n)
    }

    /// Boolean equivalence (`XNOR`) of `a` and `b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn bdd_equals(&mut self, a: BddNode, b: BddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Cudd_bddXnor(self.mgr, a.0.as_ptr(), b.0.as_ptr()) },
            "Cudd_bddXnor",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        BddNode(n)
    }

    /// Boolean inequality (`XOR`) of `a` and `b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn bdd_nequals(&mut self, a: BddNode, b: BddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Cudd_bddXor(self.mgr, a.0.as_ptr(), b.0.as_ptr()) },
            "Cudd_bddXor",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        BddNode(n)
    }

    /// Conjunction of `a` and `b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn bdd_and(&mut self, a: BddNode, b: BddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Cudd_bddAnd(self.mgr, a.0.as_ptr(), b.0.as_ptr()) },
            "Cudd_bddAnd",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        BddNode(n)
    }

    /// Disjunction of `a` and `b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn bdd_or(&mut self, a: BddNode, b: BddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Cudd_bddOr(self.mgr, a.0.as_ptr(), b.0.as_ptr()) },
            "Cudd_bddOr",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        BddNode(n)
    }

    /// Existentially abstracts variables in `cube` from `a`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a
    pub fn bdd_exists_abstract(&mut self, a: BddNode, cube: BddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Cudd_bddExistAbstract(self.mgr, a.0.as_ptr(), cube.0.as_ptr()) },
            "Cudd_bddExistAbstract",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        BddNode(n)
    }

    /// Computes `(f AND g)` then existentially abstracts variables in `cube`.\
    /// This is essentially matrix multiplication for BDDs\
    /// _Refs_: result\
    /// _Derefs_: f, g
    pub fn bdd_and_then_existsabs(&mut self, f: BddNode, g: BddNode, cube: BddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Cudd_bddAndAbstract(self.mgr, f.0.as_ptr(), g.0.as_ptr(), cube.0.as_ptr()) },
            "Cudd_bddAndAbstract",
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
    /// _Refs_: result\
    /// _Derefs_: f
    pub fn bdd_swap_variables(&mut self, f: BddNode, x: &[u16], y: &[u16]) -> BddNode {
        assert_eq!(x.len(), y.len());
        let mut xs = Vec::with_capacity(x.len());
        let mut ys = Vec::with_capacity(y.len());
        for (&xi, &yi) in x.iter().zip(y.iter()) {
            xs.push(
                self.must_node(
                    unsafe { Cudd_bddIthVar(self.mgr, xi as i32) },
                    "Cudd_bddIthVar(x)",
                )
                .as_ptr(),
            );
            ys.push(
                self.must_node(
                    unsafe { Cudd_bddIthVar(self.mgr, yi as i32) },
                    "Cudd_bddIthVar(y)",
                )
                .as_ptr(),
            );
        }
        let n = self.must_node(
            unsafe {
                Cudd_bddSwapVariables(
                    self.mgr,
                    f.0.as_ptr(),
                    xs.as_mut_ptr(),
                    ys.as_mut_ptr(),
                    x.len() as i32,
                )
            },
            "Cudd_bddSwapVariables",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        BddNode(n)
    }

    /// Swaps each variable in `x` with the corresponding variable in `y` for ADD `f`.
    ///
    /// Both index slices must have the same length.
    ///
    /// _Refs_: result\
    /// _Derefs_: f
    pub fn add_swap_vars(&mut self, f: AddNode, x: &[u16], y: &[u16]) -> AddNode {
        assert_eq!(x.len(), y.len());
        let mut xs = Vec::with_capacity(x.len());
        let mut ys = Vec::with_capacity(y.len());
        for (&xi, &yi) in x.iter().zip(y.iter()) {
            xs.push(
                self.must_node(
                    unsafe { Cudd_bddIthVar(self.mgr, xi as i32) },
                    "Cudd_bddIthVar(x)",
                )
                .as_ptr(),
            );
            ys.push(
                self.must_node(
                    unsafe { Cudd_bddIthVar(self.mgr, yi as i32) },
                    "Cudd_bddIthVar(y)",
                )
                .as_ptr(),
            );
        }
        let n = self.must_node(
            unsafe {
                Cudd_addSwapVariables(
                    self.mgr,
                    f.0.as_ptr(),
                    xs.as_mut_ptr(),
                    ys.as_mut_ptr(),
                    x.len() as i32,
                )
            },
            "Cudd_addSwapVariables",
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
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_matrix_multiply(&mut self, a: AddNode, b: AddNode, z: &[u16]) -> AddNode {
        let mut z_vars = Vec::with_capacity(z.len());
        for &zi in z {
            z_vars.push(
                self.must_node(
                    unsafe { Cudd_bddIthVar(self.mgr, zi as i32) },
                    "Cudd_bddIthVar(z)",
                )
                .as_ptr(),
            );
        }

        let n = self.must_node(
            unsafe {
                Cudd_addMatrixMultiply(
                    self.mgr,
                    a.0.as_ptr(),
                    b.0.as_ptr(),
                    z_vars.as_mut_ptr(),
                    z.len() as i32,
                )
            },
            "Cudd_addMatrixMultiply",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// Pointwise ADD addition of `a` and `b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_plus(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let n = self.add_apply(add_plus_op, a.0, b.0, "Cudd_addPlus");
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// Pointwise ADD subtraction `a - b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_minus(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let n = self.add_apply(add_minus_op, a.0, b.0, "Cudd_addMinus");
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// Pointwise ADD multiplication of `a` and `b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_times(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let n = self.add_apply(add_times_op, a.0, b.0, "Cudd_addTimes");
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// Pointwise ADD division `a / b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_divide(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let n = self.add_apply(add_divide_op, a.0, b.0, "Cudd_addDivide");
        self.ref_node(n);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// ADD `if-then-else` over `cond`, converting the condition from BDD to ADD.
    ///
    /// _Refs_: result\
    /// _Derefs_: cond, then_branch, else_branch
    pub fn add_ite(
        &mut self,
        cond: BddNode,
        then_branch: AddNode,
        else_branch: AddNode,
    ) -> AddNode {
        let cond_add = self.bdd_to_add(cond);
        let n = self.must_node(
            unsafe {
                Cudd_addIte(
                    self.mgr,
                    cond_add.0.as_ptr(),
                    then_branch.0.as_ptr(),
                    else_branch.0.as_ptr(),
                )
            },
            "Cudd_addIte",
        );
        self.ref_node(n);
        self.deref_node(cond_add.0);
        self.deref_node(then_branch.0);
        self.deref_node(else_branch.0);
        AddNode(n)
    }

    /// Existential abstraction over ADD `f` with respect to `cube`.
    ///
    /// _Refs_: result\
    /// _Derefs_: f
    pub fn add_sum_abstract(&mut self, f: AddNode, cube: AddNode) -> AddNode {
        let n = self.must_node(
            unsafe { Cudd_addExistAbstract(self.mgr, f.0.as_ptr(), cube.0.as_ptr()) },
            "Cudd_addExistAbstract",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        AddNode(n)
    }

    /// OR abstraction over ADD `f` with respect to `cube`.
    ///
    /// Assumes `f` is 0/1-valued. This corresponds to max abstraction.
    ///
    /// _Refs_: result\
    /// _Derefs_: f
    pub fn add_or_abstract(&mut self, f: AddNode, cube: AddNode) -> AddNode {
        let n = self.must_node(
            unsafe { Cudd_addOrAbstract(self.mgr, f.0.as_ptr(), cube.0.as_ptr()) },
            "Cudd_addOrAbstract",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        AddNode(n)
    }

    /// Max abstraction over ADD `f` with respect to `cube`.
    ///
    /// _Refs_: result\
    /// _Derefs_: f
    pub fn add_max_abstract(&mut self, f: AddNode, cube: AddNode) -> AddNode {
        let n = self.must_node(
            unsafe { Cudd_addMaxAbstract(self.mgr, f.0.as_ptr(), cube.0.as_ptr()) },
            "Cudd_addMaxAbstract",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        AddNode(n)
    }

    /// Min abstraction over ADD `f` with respect to `cube`.
    ///
    /// _Refs_: result\
    /// _Derefs_: f
    pub fn add_min_abstract(&mut self, f: AddNode, cube: AddNode) -> AddNode {
        let n = self.must_node(
            unsafe { Cudd_addMinAbstract(self.mgr, f.0.as_ptr(), cube.0.as_ptr()) },
            "Cudd_addMinAbstract",
        );
        self.ref_node(n);
        self.deref_node(f.0);
        AddNode(n)
    }

    /// Converts an ADD to a BDD using threshold `EPS`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a
    pub fn add_to_bdd(&mut self, a: AddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Cudd_addBddThreshold(self.mgr, a.0.as_ptr(), EPS) },
            "Cudd_addBddThreshold",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        BddNode(n)
    }

    /// Converts an ADD to its support-pattern BDD.
    ///
    /// _Refs_: result\
    /// _Derefs_: a
    pub fn add_to_bdd_pattern(&mut self, a: AddNode) -> BddNode {
        let n = self.must_node(
            unsafe { Cudd_addBddPattern(self.mgr, a.0.as_ptr()) },
            "Cudd_addBddPattern",
        );
        self.ref_node(n);
        self.deref_node(a.0);
        BddNode(n)
    }

    /// Converts a BDD to an ADD.
    ///
    /// _Refs_: result\
    /// _Derefs_: b
    pub fn bdd_to_add(&mut self, b: BddNode) -> AddNode {
        let n = self.must_node(
            unsafe { Cudd_BddToAdd(self.mgr, b.0.as_ptr()) },
            "Cudd_BddToAdd",
        );
        self.ref_node(n);
        self.deref_node(b.0);
        AddNode(n)
    }

    /// Returns BDD for `a > b` by checking positivity of `a - b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_greater_than(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let diff = self.add_minus(a, b);
        self.add_to_bdd(diff)
    }

    /// Returns BDD for `a < b` by checking positivity of `b - a`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_less_than(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let diff = self.add_minus(b, a);
        self.add_to_bdd(diff)
    }

    /// Returns BDD for `a >= b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_greater_or_equal(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let lt = self.add_less_than(a, b);
        self.bdd_not(lt)
    }

    /// Returns BDD for `a <= b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_less_or_equal(&mut self, a: AddNode, b: AddNode) -> BddNode {
        let gt = self.add_greater_than(a, b);
        self.bdd_not(gt)
    }

    /// Returns BDD for `a == b`.
    ///
    /// _Refs_: result\
    /// _Derefs_: a, b
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
    /// _Refs_: result\
    /// _Derefs_: a, b
    pub fn add_nequals(&mut self, a: AddNode, b: AddNode) -> BddNode {
        self.ref_node(a.0);
        self.ref_node(b.0);
        let gt = self.add_greater_than(a, b);
        let lt = self.add_less_than(AddNode(a.0), AddNode(b.0));
        self.bdd_or(gt, lt)
    }

    /// Returns `true` iff `|a-b|_inf <= tolerance`.
    pub fn add_equal_sup_norm(&self, a: AddNode, b: AddNode, tolerance: f64) -> bool {
        unsafe { Cudd_EqualSupNorm(self.mgr, a.0.as_ptr(), b.0.as_ptr(), tolerance, 0) != 0 }
    }

    /// Numerical epsilon used for ADD->BDD thresholding and convergence checks.
    pub fn epsilon(&self) -> f64 {
        EPS
    }

    /// Counts minterms in BDD `rel` over `num_vars` variables.
    ///
    /// _Refs_: none\
    /// _Derefs_: none
    pub fn bdd_count_minterms(&mut self, rel: BddNode, num_vars: u32) -> u64 {
        unsafe { Cudd_CountMinterm(self.mgr, rel.0.as_ptr(), num_vars as i32) }.round() as u64
    }

    /// Returns the number of DAG nodes reachable from `root`.
    pub fn dag_size(&self, root: Node) -> usize {
        let root = self.regular_node(root);
        unsafe { Cudd_DagSize(root.as_ptr()) as usize }
    }

    /// Iterates all nodes reachable from `root` and invokes `f` for each.
    pub fn foreach_node<F: FnMut(Node)>(&self, root: Node, mut f: F) {
        let root = self.regular_node(root);
        unsafe {
            Cudd_ForeachNode(self.mgr, root.as_ptr(), |n| f(Node(n)));
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
        out.sort_by_key(|n| n.0 as usize);
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
    /// _Refs_: none\
    /// _Derefs_: none
    pub fn add_stats(&mut self, root: AddNode, num_vars: u32) -> AddStats {
        let root = self.regular_node(root.0);
        let minterms =
            unsafe { Cudd_CountMinterm(self.mgr, root.as_ptr(), num_vars as i32) }.round() as u64;

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
    /// _Refs_: none\
    /// _Derefs_: none
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
    /// _Refs_: none\
    /// _Derefs_: none
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
        let add_one = self.add_const(1.0);

        for bm in 0..(1i32 << nodes.len()) {
            self.ref_node(add_one.0);
            let mut term = BddNode(add_one.0);
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

        self.deref_node(add_one.0);
        result
    }

    /// Normalizes `m` over `next_var_cube` with a zero-safe denominator.
    ///
    /// _Refs_: result\
    /// _Derefs_: m, next_var_cube
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
    /// Creates a manager using the same default CUDD initialization as [`RefManager::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RefManager {
    fn drop(&mut self) {
        if !self.mgr.is_null() {
            if ENABLE_CUDD_DEBUGCHECK_ON_DROP && !std::thread::panicking() {
                debug_assert!(
                    self.debug_check(),
                    "CUDD manager failed debug check before quit"
                );
            }
            unsafe { Cudd_Quit(self.mgr) };
            self.mgr = ptr::null_mut();
        }
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
