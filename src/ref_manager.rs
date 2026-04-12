use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Write},
};

use lumindd::{Manager, NodeId};

/// Threshold used when converting numeric ADDs to boolean BDDs.
const EPS: f64 = 1e-10;

/// Number of leak entries printed by `SymbolicDTMC::drop`.
pub static LEAK_REPORT_LIMIT: usize = 10;

/// Lightweight statistics for an ADD transition relation.
#[derive(Debug, Clone, Copy)]
pub struct AddStats {
    /// Number of unique DD nodes reachable from the root.
    pub node_count: usize,
    /// Number of unique terminal nodes reachable from the root.
    pub terminal_count: usize,
    /// Number of non-zero minterms in the corresponding 0/1 relation.
    pub minterms: u64,
}

/// Safe façade over `lumindd::Manager`.
///
/// This wrapper has two goals:
/// 1. Keep a consistent refs/derefs contract for every operation.
/// 2. Track externally owned references (`tracked_refs`) to detect leaks.
pub struct RefManager {
    inner: Manager,
    tracked_refs: HashMap<NodeId, i64>,
}

impl RefManager {
    // ---------------------------------------------------------------------
    // Lifecycle and tracked-reference bookkeeping
    // ---------------------------------------------------------------------

    /// Create a new DD manager.
    pub fn new() -> Self {
        Self {
            inner: Manager::new(),
            tracked_refs: HashMap::new(),
        }
    }

    fn track_ref(&mut self, node: NodeId) {
        self.inner.ref_node(node);
        let key = node.regular();
        *self.tracked_refs.entry(key).or_insert(0) += 1;
    }

    fn track_deref(&mut self, node: NodeId) {
        self.inner.deref_node(node);
        let key = node.regular();
        if let Some(count) = self.tracked_refs.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                self.tracked_refs.remove(&key);
            }
        }
    }

    /// Increment an externally owned reference.
    pub fn ref_node(&mut self, node: NodeId) -> NodeId {
        self.track_ref(node);
        node
    }

    /// Decrement an externally owned reference.
    pub fn deref_node(&mut self, node: NodeId) -> NodeId {
        self.track_deref(node);
        node
    }

    /// Return up to `limit` nodes with positive tracked references.
    pub fn nonzero_ref_entries(&self, limit: usize) -> Vec<(NodeId, i64)> {
        let mut entries = self
            .tracked_refs
            .iter()
            .filter_map(|(&node, &count)| if count > 0 { Some((node, count)) } else { None })
            .collect::<Vec<_>>();
        entries.sort_by_key(|(node, _)| format!("{:?}", node.regular()));
        entries.truncate(limit);
        entries
    }

    /// Number of nodes with positive tracked references.
    pub fn nonzero_ref_count(&self) -> usize {
        self.tracked_refs
            .values()
            .filter(|&&count| count > 0)
            .count()
    }

    // ---------------------------------------------------------------------
    // Read-only accessors used by analysis/debug code
    // ---------------------------------------------------------------------

    /// Variable index of a node (`u16::MAX` for terminal constants).
    pub fn read_var_index(&self, node: NodeId) -> u16 {
        self.inner.read_var_index(node)
    }

    /// Then-child accessor.
    pub fn read_then(&self, node: NodeId) -> NodeId {
        self.inner.read_then(node)
    }

    /// Else-child accessor.
    pub fn read_else(&self, node: NodeId) -> NodeId {
        self.inner.read_else(node)
    }

    /// Terminal ADD value accessor.
    pub fn add_value(&self, node: NodeId) -> Option<f64> {
        self.inner.add_value(node)
    }

    // ---------------------------------------------------------------------
    // Base constructors
    // ---------------------------------------------------------------------

    /// __Refs__: BDD ONE
    pub fn bdd_one(&mut self) -> NodeId {
        self.ref_node(NodeId::ONE);
        NodeId::ONE
    }

    /// __Refs__: BDD ZERO
    pub fn bdd_zero(&mut self) -> NodeId {
        self.ref_node(NodeId::ZERO);
        NodeId::ZERO
    }

    /// __Refs__: ADD ZERO
    pub fn add_zero(&mut self) -> NodeId {
        let node = self.inner.add_zero();
        self.ref_node(node)
    }

    /// __Refs__: Result
    pub fn add_const(&mut self, value: f64) -> NodeId {
        let node = self.inner.add_const(value);
        self.ref_node(node)
    }

    /// __Refs__: Result
    pub fn new_var(&mut self) -> NodeId {
        let node = self.inner.bdd_new_var();
        self.ref_node(node)
    }

    /// __Refs__: Result (0/1-valued ADD variable)
    pub fn add_var(&mut self, var_index: u16) -> NodeId {
        let node = self.inner.add_ith_var(var_index);
        self.ref_node(node)
    }

    // ---------------------------------------------------------------------
    // BDD operators
    // ---------------------------------------------------------------------

    /// __Refs__: Result; __Derefs__: a
    pub fn bdd_not(&mut self, a: NodeId) -> NodeId {
        let result = self.inner.bdd_not(a);
        self.ref_node(result);
        self.deref_node(a);
        result
    }

    /// __Refs__: Result; __Derefs__: a, b
    pub fn bdd_equals(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.bdd_xnor(a, b);
        self.ref_node(result);
        self.deref_node(a);
        self.deref_node(b);
        result
    }

    /// __Refs__: Result; __Derefs__: a, b
    pub fn bdd_nequals(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.bdd_xor(a, b);
        self.ref_node(result);
        self.deref_node(a);
        self.deref_node(b);
        result
    }

    /// __Refs__: Result; __Derefs__: a, b
    pub fn bdd_and(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.bdd_and(a, b);
        self.ref_node(result);
        self.deref_node(a);
        self.deref_node(b);
        result
    }

    /// __Refs__: Result; __Derefs__: a, b
    pub fn bdd_or(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.bdd_or(a, b);
        self.ref_node(result);
        self.deref_node(a);
        self.deref_node(b);
        result
    }

    /// __Refs__: Result; __Derefs__: a
    pub fn bdd_or_abstract(&mut self, a: NodeId, cube: NodeId) -> NodeId {
        let result = self.inner.bdd_exist_abstract(a, cube);
        self.ref_node(result);
        self.deref_node(a);
        result
    }

    /// __Refs__: Result; __Derefs__: f, g
    pub fn bdd_and_abstract(&mut self, f: NodeId, g: NodeId, cube: NodeId) -> NodeId {
        let result = self.inner.bdd_and_abstract(f, g, cube);
        self.ref_node(result);
        self.deref_node(f);
        self.deref_node(g);
        result
    }

    /// __Refs__: Result; __Derefs__: f
    pub fn bdd_swap_variables(&mut self, f: NodeId, x: &[u16], y: &[u16]) -> NodeId {
        let result = self.inner.bdd_swap_variables(f, x, y);
        self.ref_node(result);
        self.deref_node(f);
        result
    }

    // ---------------------------------------------------------------------
    // ADD operators
    // ---------------------------------------------------------------------

    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_plus(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.add_plus(a, b);
        self.ref_node(result);
        self.deref_node(a);
        self.deref_node(b);
        result
    }

    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_minus(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.add_minus(a, b);
        self.ref_node(result);
        self.deref_node(a);
        self.deref_node(b);
        result
    }

    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_times(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.add_times(a, b);
        self.ref_node(result);
        self.deref_node(a);
        self.deref_node(b);
        result
    }

    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_divide(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.add_divide(a, b);
        self.ref_node(result);
        self.deref_node(a);
        self.deref_node(b);
        result
    }

    /// __Refs__: Result; __Derefs__: cond, then_branch, else_branch
    pub fn add_ite(&mut self, cond: NodeId, then_branch: NodeId, else_branch: NodeId) -> NodeId {
        let result = self.inner.add_ite(cond, then_branch, else_branch);
        self.ref_node(result);
        self.deref_node(cond);
        self.deref_node(then_branch);
        self.deref_node(else_branch);
        result
    }

    /// __Refs__: Result; __Derefs__: a
    pub fn add_bdd_pattern(&mut self, a: NodeId) -> NodeId {
        let result = self.inner.add_bdd_pattern(a);
        self.ref_node(result);
        self.deref_node(a);
        result
    }

    /// __Refs__: Result; __Derefs__: f
    pub fn add_exist_abstract(&mut self, f: NodeId, cube: NodeId) -> NodeId {
        let result = self.inner.add_exist_abstract(f, cube);
        self.ref_node(result);
        self.deref_node(f);
        result
    }

    // ---------------------------------------------------------------------
    // ADD/BDD conversion and relational predicates
    // ---------------------------------------------------------------------

    /// __Refs__: Result; __Derefs__: a
    fn add_bdd_threshold(&mut self, a: NodeId, threshold: f64) -> NodeId {
        let result = self.inner.add_bdd_threshold(a, threshold);
        self.ref_node(result);
        self.deref_node(a);
        result
    }

    /// Convert ADD values `> EPS` to true.
    /// __Refs__: Result; __Derefs__: a
    pub fn add_to_bdd(&mut self, a: NodeId) -> NodeId {
        self.add_bdd_threshold(a, EPS)
    }

    /// __Refs__: Result; __Derefs__: a
    pub fn bdd_to_add(&mut self, a: NodeId) -> NodeId {
        let result = self.inner.bdd_to_add(a);
        self.ref_node(result);
        self.deref_node(a);
        result
    }

    /// Convert two ADDs to BDD: `a > b`.
    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_greater_than(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let diff = self.add_minus(a, b);
        self.add_bdd_threshold(diff, EPS)
    }

    /// Convert two ADDs to BDD: `a < b`.
    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_less_than(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let diff = self.add_minus(b, a);
        self.add_bdd_threshold(diff, EPS)
    }

    /// Convert two ADDs to BDD: `a >= b`.
    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_greater_or_equal(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let lt_bdd = self.add_less_than(a, b);
        self.bdd_not(lt_bdd)
    }

    /// Convert two ADDs to BDD: `a <= b`.
    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_less_or_equal(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let gt_bdd = self.add_greater_than(a, b);
        self.bdd_not(gt_bdd)
    }

    /// Convert two ADDs to BDD: equality.
    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_equals(&mut self, a: NodeId, b: NodeId) -> NodeId {
        self.ref_node(a);
        self.ref_node(b);
        let gt = self.add_greater_than(a, b);
        let lt = self.add_less_than(a, b);
        let neq = self.bdd_or(gt, lt);
        self.bdd_not(neq)
    }

    /// Convert two ADDs to BDD: inequality.
    /// __Refs__: Result; __Derefs__: a, b
    pub fn add_nequals(&mut self, a: NodeId, b: NodeId) -> NodeId {
        self.ref_node(a);
        self.ref_node(b);
        let gt = self.add_greater_than(a, b);
        let lt = self.add_less_than(a, b);
        self.bdd_or(gt, lt)
    }

    // ---------------------------------------------------------------------
    // Abstraction and normalization
    // ---------------------------------------------------------------------

    /// Generic ADD abstraction over vars in `cube`.
    ///
    /// For each quantified variable x, combine cofactors with `combine`.
    /// `combine` must consume its two inputs and return a referenced node.
    ///
    /// __Refs__: result; __Derefs__: f
    pub fn add_abstract_with<F>(&mut self, f: NodeId, cube: NodeId, mut combine: F) -> NodeId
    where
        F: FnMut(&mut RefManager, NodeId, NodeId) -> NodeId,
    {
        let mut vars: Vec<(u16, u32)> = self
            .inner
            .bdd_support(cube)
            .into_iter()
            .map(|var| (var, self.inner.read_perm(var)))
            .collect();
        vars.sort_by_key(|&(_, level)| level);

        let mut cache: HashMap<(NodeId, usize), NodeId> = HashMap::new();
        let res = self.add_abstract_with_rec(f.regular(), &vars, 0, &mut combine, &mut cache);

        for &cached in cache.values() {
            self.deref_node(cached);
        }
        self.deref_node(f);
        res
    }

    fn add_abstract_with_rec<F>(
        &mut self,
        node: NodeId,
        vars: &[(u16, u32)],
        idx: usize,
        combine: &mut F,
        cache: &mut HashMap<(NodeId, usize), NodeId>,
    ) -> NodeId
    where
        F: FnMut(&mut RefManager, NodeId, NodeId) -> NodeId,
    {
        let node = node.regular();
        let key = (node, idx);
        if let Some(&cached) = cache.get(&key) {
            self.ref_node(cached);
            return cached;
        }

        let result = if idx >= vars.len() {
            self.ref_node(node)
        } else {
            let node_var = self.inner.read_var_index(node);
            if node_var == u16::MAX {
                let mut acc = self.ref_node(node);
                for _ in idx..vars.len() {
                    self.ref_node(acc);
                    self.ref_node(acc);
                    let next = combine(self, acc, acc);
                    self.deref_node(acc);
                    acc = next;
                }
                acc
            } else {
                let node_level = self.inner.read_perm(node_var);
                let (_, quant_level) = vars[idx];

                if node_level > quant_level {
                    self.ref_node(node);
                    self.ref_node(node);
                    let merged = combine(self, node, node);
                    let out = self.add_abstract_with_rec(merged, vars, idx + 1, combine, cache);
                    self.deref_node(merged);
                    out
                } else if node_level == quant_level {
                    let t = self.inner.read_then(node).regular();
                    let e = self.inner.read_else(node).regular();
                    self.ref_node(t);
                    self.ref_node(e);
                    let merged = combine(self, t, e);
                    let out = self.add_abstract_with_rec(merged, vars, idx + 1, combine, cache);
                    self.deref_node(merged);
                    out
                } else {
                    let t = self.inner.read_then(node).regular();
                    let e = self.inner.read_else(node).regular();
                    let t_abs = self.add_abstract_with_rec(t, vars, idx, combine, cache);
                    let e_abs = self.add_abstract_with_rec(e, vars, idx, combine, cache);
                    let cond = self.add_var(node_var);
                    self.add_ite(cond, t_abs, e_abs)
                }
            }
        };

        self.ref_node(result);
        cache.insert(key, result);
        result
    }

    /// Sum-abstraction over all vars in `cube`.
    /// __Refs__: result; __Derefs__: f
    pub fn add_sum_abstract(&mut self, f: NodeId, cube: NodeId) -> NodeId {
        self.add_abstract_with(f, cube, |mgr, a, b| mgr.add_plus(a, b))
    }

    // ---------------------------------------------------------------------
    // Counting/statistics
    // ---------------------------------------------------------------------

    /// Count BDD minterms using the given number of support variables.\
    /// __Refs__: None; __Derefs__: None
    pub fn bdd_count_minterms(&mut self, bdd: NodeId, num_vars: u32) -> u64 {
        self.inner.bdd_count_minterm(bdd, num_vars).round() as u64
    }

    /// Count unique nodes in a DD.
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
        if self.inner.read_var_index(node) == u16::MAX {
            return;
        }
        let t = self.inner.read_then(node);
        let e = self.inner.read_else(node);
        self.num_nodes_rec(t, visited);
        self.num_nodes_rec(e, visited);
    }

    /// Combined ADD stats used in CLI summaries.
    pub fn add_stats(&mut self, root: NodeId, num_vars: u32) -> AddStats {
        let root = root.regular();

        let mut visited = HashSet::new();
        let mut terminal_count = 0usize;
        self.count_nodes_and_terminals(root, &mut visited, &mut terminal_count);

        self.ref_node(root);
        let bdd = self.add_to_bdd(root);
        let minterms = self.bdd_count_minterms(bdd, num_vars);
        self.deref_node(bdd);

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
        if self.inner.read_var_index(node) == u16::MAX {
            *terminal_count += 1;
            return;
        }
        let t = self.inner.read_then(node);
        let e = self.inner.read_else(node);
        self.count_nodes_and_terminals(t, visited, terminal_count);
        self.count_nodes_and_terminals(e, visited, terminal_count);
    }

    // ---------------------------------------------------------------------
    // DOT export helpers
    // ---------------------------------------------------------------------

    fn var_index_label_map(&self, var_names: &HashMap<NodeId, String>) -> HashMap<u16, String> {
        let mut labels = HashMap::new();
        for (&node, name) in var_names {
            let var_index = self.inner.read_var_index(node.regular());
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

    /// Export an ADD root to Graphviz DOT.
    pub fn dump_add_dot(
        &self,
        root: NodeId,
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
            root.regular(),
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
        let var = self.inner.read_var_index(n);
        if var == u16::MAX {
            let v = self.inner.add_value(n).unwrap_or(f64::NAN);
            writeln!(out, "  n{} [shape=box,label=\"{}\"] ;", this, v)?;
            return Ok(());
        }

        let t = self.inner.read_then(n).regular();
        let e = self.inner.read_else(n).regular();
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

    /// Export a BDD root to Graphviz DOT.
    pub fn dump_bdd_dot(
        &self,
        root: NodeId,
        path: &str,
        var_names: &HashMap<NodeId, String>,
    ) -> io::Result<()> {
        let mut out = File::create(path)?;
        writeln!(out, "digraph BDD {{")?;
        writeln!(out, "  rankdir=TB;")?;
        writeln!(out, "  ONE [shape=box,label=\"1\"];")?;
        writeln!(out, "  ZERO [shape=box,label=\"0\"];")?;

        if root.is_one() {
            writeln!(out, "  root [shape=point];")?;
            writeln!(out, "  root -> ONE;")?;
            writeln!(out, "}}")?;
            return Ok(());
        }
        if root.is_zero() {
            writeln!(out, "  root [shape=point];")?;
            writeln!(out, "  root -> ZERO;")?;
            writeln!(out, "}}")?;
            return Ok(());
        }

        let labels = self.var_index_label_map(var_names);
        let mut ids: HashMap<NodeId, usize> = HashMap::new();
        let mut next_id = 0usize;
        let mut visited: HashSet<NodeId> = HashSet::new();

        let root_reg = root.regular();
        let root_id = Self::intern_id(&mut ids, &mut next_id, root_reg);
        writeln!(out, "  root [shape=point];")?;
        if root.is_complemented() {
            writeln!(out, "  root -> n{} [color=red];", root_id)?;
        } else {
            writeln!(out, "  root -> n{};", root_id)?;
        }

        self.dump_bdd_dot_rec(
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

    fn dump_bdd_dot_rec<W: Write>(
        &self,
        n: NodeId,
        out: &mut W,
        labels: &HashMap<u16, String>,
        ids: &mut HashMap<NodeId, usize>,
        next_id: &mut usize,
        visited: &mut HashSet<NodeId>,
    ) -> io::Result<()> {
        let n = n.regular();
        if n.is_constant() || !visited.insert(n) {
            return Ok(());
        }

        let this = Self::intern_id(ids, next_id, n);
        let var = self.inner.read_var_index(n);
        let label = Self::var_label(var, labels);
        writeln!(out, "  n{} [shape=ellipse,label=\"{}\"] ;", this, label)?;

        let t = self.inner.read_then(n);
        let e = self.inner.read_else(n);

        let t_target = if t.is_one() {
            "ONE".to_string()
        } else if t.is_zero() {
            "ZERO".to_string()
        } else {
            format!("n{}", Self::intern_id(ids, next_id, t.regular()))
        };

        let e_target = if e.is_one() {
            "ONE".to_string()
        } else if e.is_zero() {
            "ZERO".to_string()
        } else {
            format!("n{}", Self::intern_id(ids, next_id, e.regular()))
        };

        if t.is_complemented() {
            writeln!(out, "  n{} -> {} [label=\"1\",color=red];", this, t_target)?;
        } else {
            writeln!(out, "  n{} -> {} [label=\"1\"] ;", this, t_target)?;
        }

        if e.is_complemented() {
            writeln!(
                out,
                "  n{} -> {} [label=\"0\",style=dashed,color=red];",
                this, e_target
            )?;
        } else {
            writeln!(
                out,
                "  n{} -> {} [label=\"0\",style=dashed,color=blue];",
                this, e_target
            )?;
        }

        if !t.is_constant() {
            self.dump_bdd_dot_rec(t.regular(), out, labels, ids, next_id, visited)?;
        }
        if !e.is_constant() {
            self.dump_bdd_dot_rec(e.regular(), out, labels, ids, next_id, visited)?;
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Utilities
    // ---------------------------------------------------------------------

    /// Build an ADD that decodes a bit-vector assignment to its integer value.
    ///
    /// `nodes[0]` is treated as the LSB.
    pub fn get_encoding(&mut self, nodes: &[NodeId]) -> NodeId {
        let mut result = self.add_const(0.0);
        let add_one = self.add_const(1.0);

        for bm in 0..(1i32 << nodes.len()) {
            self.ref_node(add_one);
            let mut term = add_one;
            for (i, &var) in nodes.iter().enumerate() {
                self.ref_node(var);
                let literal = if (bm & (1 << i)) != 0 {
                    var
                } else {
                    self.bdd_not(var)
                };
                term = self.bdd_and(term, literal);
            }
            let term = self.bdd_to_add(term);
            let value = self.add_const(bm as f64);
            let term = self.add_times(term, value);
            result = self.add_plus(result, term);
        }

        self.deref_node(add_one);
        result
    }

    /// Normalize row-wise probabilities over next-state variables:
    /// `m / Abstract(+, y, m)` with safe division on out-of-domain rows.
    /// __Refs__: result; __Derefs__: m
    pub fn unif(&mut self, m: NodeId, next_var_cube: NodeId) -> NodeId {
        self.ref_node(m);
        let denom = self.add_sum_abstract(m, next_var_cube);

        self.ref_node(denom);
        let denom_pos_bdd = self.add_bdd_threshold(denom, EPS);
        let denom_pos_add = self.bdd_to_add(denom_pos_bdd);
        let one = self.add_const(1.0);
        let denom_is_zero_add = self.add_minus(one, denom_pos_add);
        let safe_denom = self.add_plus(denom, denom_is_zero_add);

        self.add_divide(m, safe_denom)
    }
}
