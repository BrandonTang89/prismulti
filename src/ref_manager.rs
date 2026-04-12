use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Write},
    ptr,
};

use cudd_sys::{
    DdManager, DdNode,
    cudd::{
        CUDD_CACHE_SLOTS, CUDD_UNIQUE_SLOTS, Cudd_BddToAdd, Cudd_CountMinterm, Cudd_E,
        Cudd_IsComplement, Cudd_IsConstant, Cudd_NodeReadIndex, Cudd_Not, Cudd_Quit,
        Cudd_ReadLogicZero, Cudd_ReadOne, Cudd_RecursiveDeref, Cudd_Ref, Cudd_Regular, Cudd_T,
        Cudd_V, Cudd_addApply, Cudd_addBddPattern, Cudd_addBddThreshold, Cudd_addConst,
        Cudd_addDivide, Cudd_addExistAbstract, Cudd_addIte, Cudd_addIthVar, Cudd_addMinus,
        Cudd_addPlus, Cudd_addTimes, Cudd_bddAnd, Cudd_bddAndAbstract, Cudd_bddExistAbstract,
        Cudd_bddIthVar, Cudd_bddNewVar, Cudd_bddOr, Cudd_bddSwapVariables, Cudd_bddXnor,
        Cudd_bddXor, DD_APPLY_OPERATOR,
    },
};

/// Threshold used for ADD -> 0/1 conversion.
const EPS: f64 = 1e-10;

/// Number of leak entries printed by `SymbolicDTMC::drop`.
pub static LEAK_REPORT_LIMIT: usize = 10;

/// Lightweight statistics for an ADD transition relation.
#[derive(Debug, Clone, Copy)]
pub struct AddStats {
    pub node_count: usize,
    pub terminal_count: usize,
    pub minterms: u64,
}

/// Raw DD node handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(*mut DdNode);

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "N{:x}", self.regular().raw_index())
    }
}

impl NodeId {
    #[inline]
    pub fn regular(self) -> Self {
        Self(unsafe { Cudd_Regular(self.0) })
    }

    #[inline]
    pub fn is_complemented(self) -> bool {
        unsafe { Cudd_IsComplement(self.0) != 0 }
    }

    #[inline]
    pub fn is_constant(self) -> bool {
        unsafe { Cudd_IsConstant(self.regular().0) != 0 }
    }

    #[inline]
    pub fn raw_index(self) -> usize {
        (self.regular().0 as usize) >> 1
    }

    #[inline]
    fn as_ptr(self) -> *mut DdNode {
        self.0
    }
}

/// General numeric ADD node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AddNode(pub NodeId);

/// Boolean relation/set encoded as a 0-1 decision diagram.
///
/// Hierarchy:
/// - `NodeId`: raw manager node
/// - `AddNode`: arbitrary numeric ADD
/// - `Add01Node`: 0-1 relation/set (semantic subtype)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Add01Node(pub NodeId);

/// Safe facade over CUDD manager with explicit ref accounting.
///
/// Ownership convention used in doc comments:
/// - `__Refs__`: externally owned references created by this call.
/// - `__Derefs__`: input references consumed by this call.
pub struct RefManager {
    mgr: *mut DdManager,
    tracked_refs: HashMap<NodeId, i64>,
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
    /// Create a new CUDD manager.
    pub fn new() -> Self {
        let mgr =
            unsafe { cudd_sys::cudd::Cudd_Init(0, 0, CUDD_UNIQUE_SLOTS, CUDD_CACHE_SLOTS, 0) };
        assert!(!mgr.is_null(), "Failed to initialize CUDD manager");
        Self {
            mgr,
            tracked_refs: HashMap::new(),
        }
    }

    fn one_nodeid(&self) -> NodeId {
        NodeId(unsafe { Cudd_ReadOne(self.mgr) })
    }

    fn zero01_nodeid(&self) -> NodeId {
        NodeId(unsafe { Cudd_ReadLogicZero(self.mgr) })
    }

    fn must_node(&self, p: *mut DdNode, op: &str) -> NodeId {
        assert!(!p.is_null(), "CUDD returned NULL in {op}");
        NodeId(p)
    }

    fn add_apply(&self, op: DD_APPLY_OPERATOR, a: NodeId, b: NodeId, op_name: &str) -> NodeId {
        self.must_node(
            unsafe { Cudd_addApply(self.mgr, op, a.as_ptr(), b.as_ptr()) },
            op_name,
        )
    }

    fn track_ref(&mut self, node: NodeId) {
        let key = node.regular();
        unsafe { Cudd_Ref(key.as_ptr()) };
        *self.tracked_refs.entry(key).or_insert(0) += 1;
    }

    fn track_deref(&mut self, node: NodeId) {
        let key = node.regular();
        unsafe { Cudd_RecursiveDeref(self.mgr, key.as_ptr()) };
        if let Some(count) = self.tracked_refs.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                self.tracked_refs.remove(&key);
            }
        }
    }

    /// Increment an externally owned reference.
    /// __Refs__: `node`; __Derefs__: none.
    pub fn ref_node(&mut self, node: NodeId) -> NodeId {
        self.track_ref(node);
        node
    }

    /// Decrement an externally owned reference.
    /// __Refs__: none; __Derefs__: `node`.
    pub fn deref_node(&mut self, node: NodeId) -> NodeId {
        self.track_deref(node);
        node
    }

    pub fn nonzero_ref_entries(&self, limit: usize) -> Vec<(NodeId, i64)> {
        let mut entries = self
            .tracked_refs
            .iter()
            .filter_map(|(&node, &count)| if count > 0 { Some((node, count)) } else { None })
            .collect::<Vec<_>>();
        entries.sort_by_key(|(node, _)| node.raw_index());
        entries.truncate(limit);
        entries
    }

    pub fn nonzero_ref_count(&self) -> usize {
        self.tracked_refs
            .values()
            .filter(|&&count| count > 0)
            .count()
    }

    pub fn read_var_index(&self, node: NodeId) -> u16 {
        let reg = node.regular();
        if reg.is_constant() {
            u16::MAX
        } else {
            unsafe { Cudd_NodeReadIndex(reg.as_ptr()) as u16 }
        }
    }

    pub fn read_then(&self, node: NodeId) -> NodeId {
        let reg = node.regular();
        if reg.is_constant() {
            reg
        } else {
            NodeId(unsafe { Cudd_T(reg.as_ptr()) })
        }
    }

    pub fn read_else(&self, node: NodeId) -> NodeId {
        let reg = node.regular();
        if reg.is_constant() {
            reg
        } else {
            let e = NodeId(unsafe { Cudd_E(reg.as_ptr()) });
            if node.is_complemented() {
                NodeId(unsafe { Cudd_Not(e.as_ptr()) })
            } else {
                e
            }
        }
    }

    pub fn add_value(&self, node: NodeId) -> Option<f64> {
        let reg = node.regular();
        if reg.is_constant() {
            let v = unsafe { Cudd_V(reg.as_ptr()) };
            if node.is_complemented() {
                Some(1.0 - v)
            } else {
                Some(v)
            }
        } else {
            None
        }
    }

    /// __Refs__: result; __Derefs__: none.
    pub fn add01_one(&mut self) -> Add01Node {
        Add01Node(self.ref_node(self.one_nodeid()))
    }

    /// __Refs__: result; __Derefs__: none.
    pub fn add01_zero(&mut self) -> Add01Node {
        Add01Node(self.ref_node(self.zero01_nodeid()))
    }

    /// __Refs__: result; __Derefs__: none.
    pub fn add_zero(&mut self) -> AddNode {
        self.add_const(0.0)
    }

    /// __Refs__: result; __Derefs__: none.
    pub fn add_const(&mut self, value: f64) -> AddNode {
        let node = self.must_node(unsafe { Cudd_addConst(self.mgr, value) }, "Cudd_addConst");
        AddNode(self.ref_node(node))
    }

    /// Create a new variable and return it as a 0-1 ADD literal.
    /// __Refs__: result; __Derefs__: none.
    pub fn new_var(&mut self) -> Add01Node {
        let node = self.must_node(unsafe { Cudd_bddNewVar(self.mgr) }, "Cudd_bddNewVar");
        Add01Node(self.ref_node(node))
    }

    /// __Refs__: result; __Derefs__: none.
    pub fn add_var(&mut self, var_index: u16) -> Add01Node {
        let node = self.must_node(
            unsafe { Cudd_addIthVar(self.mgr, var_index as i32) },
            "Cudd_addIthVar",
        );
        Add01Node(self.ref_node(node))
    }

    /// __Refs__: result; __Derefs__: `a`.
    pub fn add01_not(&mut self, a: Add01Node) -> Add01Node {
        let result = NodeId(unsafe { Cudd_Not(a.0.as_ptr()) });
        self.ref_node(result);
        self.deref_node(a.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add01_equals(&mut self, a: Add01Node, b: Add01Node) -> Add01Node {
        let result = self.must_node(
            unsafe { Cudd_bddXnor(self.mgr, a.0.as_ptr(), b.0.as_ptr()) },
            "Cudd_bddXnor",
        );
        self.ref_node(result);
        self.deref_node(a.0);
        self.deref_node(b.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add01_nequals(&mut self, a: Add01Node, b: Add01Node) -> Add01Node {
        let result = self.must_node(
            unsafe { Cudd_bddXor(self.mgr, a.0.as_ptr(), b.0.as_ptr()) },
            "Cudd_bddXor",
        );
        self.ref_node(result);
        self.deref_node(a.0);
        self.deref_node(b.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add01_and(&mut self, a: Add01Node, b: Add01Node) -> Add01Node {
        let result = self.must_node(
            unsafe { Cudd_bddAnd(self.mgr, a.0.as_ptr(), b.0.as_ptr()) },
            "Cudd_bddAnd",
        );
        self.ref_node(result);
        self.deref_node(a.0);
        self.deref_node(b.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add01_or(&mut self, a: Add01Node, b: Add01Node) -> Add01Node {
        let result = self.must_node(
            unsafe { Cudd_bddOr(self.mgr, a.0.as_ptr(), b.0.as_ptr()) },
            "Cudd_bddOr",
        );
        self.ref_node(result);
        self.deref_node(a.0);
        self.deref_node(b.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `a`.
    pub fn add01_or_abstract(&mut self, a: Add01Node, cube: Add01Node) -> Add01Node {
        let result = self.must_node(
            unsafe { Cudd_bddExistAbstract(self.mgr, a.0.as_ptr(), cube.0.as_ptr()) },
            "Cudd_bddExistAbstract",
        );
        self.ref_node(result);
        self.deref_node(a.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `f`, `g`.
    pub fn add01_and_abstract(&mut self, f: Add01Node, g: Add01Node, cube: Add01Node) -> Add01Node {
        let result = self.must_node(
            unsafe { Cudd_bddAndAbstract(self.mgr, f.0.as_ptr(), g.0.as_ptr(), cube.0.as_ptr()) },
            "Cudd_bddAndAbstract",
        );
        self.ref_node(result);
        self.deref_node(f.0);
        self.deref_node(g.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `f`.
    pub fn add01_swap_variables(&mut self, f: Add01Node, x: &[u16], y: &[u16]) -> Add01Node {
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
        let result = self.must_node(
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
        self.ref_node(result);
        self.deref_node(f.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_plus(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let result = self.add_apply(add_plus_op, a.0, b.0, "Cudd_addPlus");
        self.ref_node(result);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(result)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_minus(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let result = self.add_apply(add_minus_op, a.0, b.0, "Cudd_addMinus");
        self.ref_node(result);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(result)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_times(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let result = self.add_apply(add_times_op, a.0, b.0, "Cudd_addTimes");
        self.ref_node(result);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(result)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_divide(&mut self, a: AddNode, b: AddNode) -> AddNode {
        let result = self.add_apply(add_divide_op, a.0, b.0, "Cudd_addDivide");
        self.ref_node(result);
        self.deref_node(a.0);
        self.deref_node(b.0);
        AddNode(result)
    }

    /// __Refs__: result; __Derefs__: `cond`, `then_branch`, `else_branch`.
    pub fn add_ite(
        &mut self,
        cond: Add01Node,
        then_branch: AddNode,
        else_branch: AddNode,
    ) -> AddNode {
        let result = self.must_node(
            unsafe {
                Cudd_addIte(
                    self.mgr,
                    cond.0.as_ptr(),
                    then_branch.0.as_ptr(),
                    else_branch.0.as_ptr(),
                )
            },
            "Cudd_addIte",
        );
        self.ref_node(result);
        self.deref_node(cond.0);
        self.deref_node(then_branch.0);
        self.deref_node(else_branch.0);
        AddNode(result)
    }

    /// __Refs__: result; __Derefs__: `a`.
    pub fn add01_from_add_pattern(&mut self, a: AddNode) -> Add01Node {
        let result = self.must_node(
            unsafe { Cudd_addBddPattern(self.mgr, a.0.as_ptr()) },
            "Cudd_addBddPattern",
        );
        self.ref_node(result);
        self.deref_node(a.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `f`.
    pub fn add_exist_abstract(&mut self, f: AddNode, cube: Add01Node) -> AddNode {
        self.ref_node(cube.0);
        let cube_add = self.add01_to_add(cube);
        let result = self.must_node(
            unsafe { Cudd_addExistAbstract(self.mgr, f.0.as_ptr(), cube_add.0.as_ptr()) },
            "Cudd_addExistAbstract",
        );
        self.ref_node(result);
        self.deref_node(f.0);
        self.deref_node(cube_add.0);
        AddNode(result)
    }

    /// __Refs__: result; __Derefs__: `a`.
    pub fn add01_from_add_threshold(&mut self, a: AddNode, threshold: f64) -> Add01Node {
        let result = self.must_node(
            unsafe { Cudd_addBddThreshold(self.mgr, a.0.as_ptr(), threshold) },
            "Cudd_addBddThreshold",
        );
        self.ref_node(result);
        self.deref_node(a.0);
        Add01Node(result)
    }

    /// __Refs__: result; __Derefs__: `a`.
    pub fn add01_from_add(&mut self, a: AddNode) -> Add01Node {
        self.add01_from_add_threshold(a, EPS)
    }

    /// __Refs__: result; __Derefs__: `a`.
    pub fn add01_to_add(&mut self, a: Add01Node) -> AddNode {
        let result = self.must_node(
            unsafe { Cudd_BddToAdd(self.mgr, a.0.as_ptr()) },
            "Cudd_BddToAdd",
        );
        self.ref_node(result);
        self.deref_node(a.0);
        AddNode(result)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_greater_than(&mut self, a: AddNode, b: AddNode) -> Add01Node {
        let diff = self.add_minus(a, b);
        self.add01_from_add_threshold(diff, EPS)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_less_than(&mut self, a: AddNode, b: AddNode) -> Add01Node {
        let diff = self.add_minus(b, a);
        self.add01_from_add_threshold(diff, EPS)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_greater_or_equal(&mut self, a: AddNode, b: AddNode) -> Add01Node {
        let lt = self.add_less_than(a, b);
        self.add01_not(lt)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_less_or_equal(&mut self, a: AddNode, b: AddNode) -> Add01Node {
        let gt = self.add_greater_than(a, b);
        self.add01_not(gt)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_equals(&mut self, a: AddNode, b: AddNode) -> Add01Node {
        self.ref_node(a.0);
        self.ref_node(b.0);
        let gt = self.add_greater_than(a, b);
        let lt = self.add_less_than(AddNode(a.0), AddNode(b.0));
        let neq = self.add01_or(gt, lt);
        self.add01_not(neq)
    }

    /// __Refs__: result; __Derefs__: `a`, `b`.
    pub fn add_nequals(&mut self, a: AddNode, b: AddNode) -> Add01Node {
        self.ref_node(a.0);
        self.ref_node(b.0);
        let gt = self.add_greater_than(a, b);
        let lt = self.add_less_than(AddNode(a.0), AddNode(b.0));
        self.add01_or(gt, lt)
    }

    /// __Refs__: result; __Derefs__: `f`.
    pub fn add_sum_abstract(&mut self, f: AddNode, cube: Add01Node) -> AddNode {
        self.ref_node(cube.0);
        let cube_add = self.add01_to_add(cube);
        let result = self.must_node(
            unsafe { Cudd_addExistAbstract(self.mgr, f.0.as_ptr(), cube_add.0.as_ptr()) },
            "Cudd_addExistAbstract(sum)",
        );
        self.ref_node(result);
        self.deref_node(f.0);
        self.deref_node(cube_add.0);
        AddNode(result)
    }

    pub fn add01_count_minterms(&mut self, rel: Add01Node, num_vars: u32) -> u64 {
        unsafe { Cudd_CountMinterm(self.mgr, rel.0.as_ptr(), num_vars as i32) }.round() as u64
    }

    pub fn num_nodes(&self, node: NodeId) -> usize {
        let mut visited = HashSet::new();
        self.num_nodes_rec(node.regular(), &mut visited);
        visited.len()
    }

    fn num_nodes_rec(&self, node: NodeId, visited: &mut HashSet<NodeId>) {
        let node = node.regular();
        if !visited.insert(node) {
            return;
        }
        if self.read_var_index(node) == u16::MAX {
            return;
        }
        self.num_nodes_rec(self.read_then(node), visited);
        self.num_nodes_rec(self.read_else(node), visited);
    }

    pub fn add_stats(&mut self, root: AddNode, num_vars: u32) -> AddStats {
        let root = root.0.regular();
        let mut visited = HashSet::new();
        let mut terminal_count = 0usize;
        self.count_nodes_and_terminals(root, &mut visited, &mut terminal_count);

        self.ref_node(root);
        let rel = self.add01_from_add(AddNode(root));
        let minterms = self.add01_count_minterms(rel, num_vars);
        self.deref_node(rel.0);

        AddStats {
            node_count: visited.len(),
            terminal_count,
            minterms,
        }
    }

    fn count_nodes_and_terminals(
        &self,
        node: NodeId,
        visited: &mut HashSet<NodeId>,
        terminal_count: &mut usize,
    ) {
        let node = node.regular();
        if !visited.insert(node) {
            return;
        }
        if self.read_var_index(node) == u16::MAX {
            *terminal_count += 1;
            return;
        }
        self.count_nodes_and_terminals(self.read_then(node), visited, terminal_count);
        self.count_nodes_and_terminals(self.read_else(node), visited, terminal_count);
    }

    fn var_index_label_map(&self, var_names: &HashMap<NodeId, String>) -> HashMap<u16, String> {
        let mut labels = HashMap::new();
        for (&node, name) in var_names {
            let var_index = self.read_var_index(node.regular());
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

    fn intern_id(ids: &mut HashMap<NodeId, usize>, next_id: &mut usize, n: NodeId) -> usize {
        *ids.entry(n.regular()).or_insert_with(|| {
            let id = *next_id;
            *next_id += 1;
            id
        })
    }

    pub fn dump_add_dot(
        &self,
        root: AddNode,
        path: &str,
        var_names: &HashMap<NodeId, String>,
    ) -> io::Result<()> {
        let mut out = File::create(path)?;
        writeln!(out, "digraph ADD {{")?;
        writeln!(out, "  rankdir=TB;")?;

        let mut ids: HashMap<NodeId, usize> = HashMap::new();
        let mut next_id = 0usize;
        let mut visited: HashSet<NodeId> = HashSet::new();
        let labels = self.var_index_label_map(var_names);

        self.dump_add_dot_rec(
            root.0.regular(),
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
        n: NodeId,
        out: &mut W,
        labels: &HashMap<u16, String>,
        ids: &mut HashMap<NodeId, usize>,
        next_id: &mut usize,
        visited: &mut HashSet<NodeId>,
    ) -> io::Result<()> {
        let n = n.regular();
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

        let t = self.read_then(n).regular();
        let e = self.read_else(n).regular();
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

    pub fn dump_add01_dot(
        &self,
        root: Add01Node,
        path: &str,
        var_names: &HashMap<NodeId, String>,
    ) -> io::Result<()> {
        self.dump_add_dot(AddNode(root.0), path, var_names)
    }

    pub fn get_encoding(&mut self, nodes: &[NodeId]) -> AddNode {
        let mut result = self.add_const(0.0);
        let add_one = self.add_const(1.0);

        for bm in 0..(1i32 << nodes.len()) {
            self.ref_node(add_one.0);
            let mut term = Add01Node(add_one.0);
            for (i, &var) in nodes.iter().enumerate() {
                self.ref_node(var);
                let literal = if (bm & (1 << i)) != 0 {
                    Add01Node(var)
                } else {
                    self.add01_not(Add01Node(var))
                };
                term = self.add01_and(term, literal);
            }
            let term = self.add01_to_add(term);
            let value = self.add_const(bm as f64);
            let term = self.add_times(term, value);
            result = self.add_plus(result, term);
        }

        self.deref_node(add_one.0);
        result
    }

    pub fn unif(&mut self, m: AddNode, next_var_cube: Add01Node) -> AddNode {
        self.ref_node(m.0);
        let denom = self.add_sum_abstract(m, next_var_cube);

        self.ref_node(denom.0);
        let denom_pos = self.add01_from_add_threshold(denom, EPS);
        let denom_pos_add = self.add01_to_add(denom_pos);
        let one = self.add_const(1.0);
        let denom_is_zero_add = self.add_minus(one, denom_pos_add);
        let safe_denom = self.add_plus(denom, denom_is_zero_add);

        self.add_divide(m, safe_denom)
    }
}

impl Drop for RefManager {
    fn drop(&mut self) {
        if !self.mgr.is_null() {
            unsafe { Cudd_Quit(self.mgr) };
            self.mgr = ptr::null_mut();
        }
    }
}
