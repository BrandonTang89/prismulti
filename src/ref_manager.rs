use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Write},
};

use lumindd::{Manager, NodeId};
pub struct RefManager {
    pub inner: Manager,
}

static EPS: f64 = 1e-10;

impl RefManager {
    pub fn new() -> Self {
        Self {
            inner: Manager::new(),
        }
    }

    /// __Refs__: Result \
    pub fn add_const(&mut self, value: f64) -> NodeId {
        let node = self.inner.add_const(value);
        self.inner.ref_node(node);
        node
    }

    /// __Refs__: Result \
    /// __Derefs__: a
    pub fn bdd_not(&mut self, a: NodeId) -> NodeId {
        let result = self.inner.bdd_not(a);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn bdd_equals(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.bdd_xnor(a, b);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        self.inner.deref_node(b);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn bdd_nequals(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.bdd_xor(a, b);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        self.inner.deref_node(b);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn bdd_and(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.bdd_and(a, b);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        self.inner.deref_node(b);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_times(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.add_times(a, b);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        self.inner.deref_node(b);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_divide(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.add_divide(a, b);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        self.inner.deref_node(b);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_plus(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.add_plus(a, b);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        self.inner.deref_node(b);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_minus(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let result = self.inner.add_minus(a, b);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        self.inner.deref_node(b);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a
    pub fn add_bdd_pattern(&mut self, a: NodeId) -> NodeId {
        let result = self.inner.add_bdd_pattern(a);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a
    pub fn add_bdd_threshold(&mut self, a: NodeId, threshold: f64) -> NodeId {
        let result = self.inner.add_bdd_threshold(a, threshold);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        result
    }

    /// __Refs__: Result \
    /// __Derefs__: a
    pub fn bdd_to_add(&mut self, a: NodeId) -> NodeId {
        let result = self.inner.bdd_to_add(a);
        self.inner.ref_node(result);
        self.inner.deref_node(a);
        result
    }

    /// Convert two ADDs to a BDD that is 1 iff they are equal.
    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_equals(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let diff = self.add_minus(a, b);
        let neq = self.add_bdd_threshold(diff, EPS);
        self.bdd_not(neq)
    }

    /// Convert two ADDs to a BDD that is 1 iff they are not equal.
    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_nequals(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let diff = self.add_minus(a, b);
        self.add_bdd_threshold(diff, EPS)
    }

    /// Convert two ADDs to a BDD that is 1 iff a > b.
    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_greater_than(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let diff = self.add_minus(a, b);
        self.add_bdd_threshold(diff, EPS)
    }

    /// Convert two ADDs to a BDD that is 1 iff a < b.
    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_less_than(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let diff = self.add_minus(b, a);
        self.add_bdd_threshold(diff, EPS)
    }

    /// Convert two ADDs to a BDD that is 1 iff a >= b.
    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_greater_or_equal(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let lt = self.add_less_than(a, b);
        self.bdd_not(lt)
    }

    /// Convert two ADDs to a BDD that is 1 iff a <= b.
    /// __Refs__: Result \
    /// __Derefs__: a, b
    pub fn add_less_or_equal(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let gt = self.add_greater_than(a, b);
        self.bdd_not(gt)
    }

    /// __Refs__: Result \
    /// __Derefs__: F
    pub fn add_exist_abstract(&mut self, f: NodeId, cube: NodeId) -> NodeId {
        let result = self.inner.add_exist_abstract(f, cube);
        self.inner.ref_node(result);
        self.inner.deref_node(f);
        result
    }

    /// __Refs__: Result
    pub fn bdd_new_var(&mut self) -> NodeId {
        let node = self.inner.bdd_new_var();
        self.inner.ref_node(node);
        node
    }

    /// __Refs__: Node \
    /// __Derefs__: None
    pub fn ref_node(&mut self, node: NodeId) -> NodeId {
        self.inner.ref_node(node);
        node
    }

    /// __Refs__: None \
    /// __Derefs__: Node
    pub fn deref_node(&mut self, node: NodeId) -> NodeId {
        self.inner.deref_node(node);
        node
    }

    /// __Refs__: ONE \
    pub fn one(&mut self) -> NodeId {
        self.inner.ref_node(NodeId::ONE);
        NodeId::ONE
    }

    /// __Refs__: ZERO \
    pub fn zero(&mut self) -> NodeId {
        self.inner.ref_node(NodeId::ZERO);
        NodeId::ZERO
    }

    /// Unif(m) = m ÷ Abstract(+,next_var_cube,m). \
    /// __Refs__: result \
    /// __Derefs__: m
    pub fn unif(&mut self, m: NodeId, next_var_cube: NodeId) -> NodeId {
        self.ref_node(m);
        let tmp = self.add_exist_abstract(m, next_var_cube);
        let res = self.add_divide(m, tmp);
        res
    }

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

    /// Dumps a graphviz dot file representing the structure of the ADD rooted at `root`. \
    /// Positive edges (then) are solid, negative edges (else) are dashed. \
    /// __Refs__: None \
    /// __Derefs__: None
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
    fn intern_id(ids: &mut HashMap<NodeId, usize>, next_id: &mut usize, n: NodeId) -> usize {
        *ids.entry(n.regular()).or_insert_with(|| {
            let id = *next_id;
            *next_id += 1;
            id
        })
    }

    /// Recurses through the DD structure to output DOT \
    /// Uses ids and next_id to assign a unique integer to each node created to use as the
    /// node identifier in the dot file \
    /// Uses labels to label the nodes in the dot file \
    /// We don't use the labels as identifiers since the labels can contain spaces
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
            writeln!(out, "  n{} [shape=box,label=\"{}\"];", this, v)?;
            return Ok(());
        }
        let t = self.inner.read_then(n).regular();
        let e = self.inner.read_else(n).regular();
        let tid = Self::intern_id(ids, next_id, t);
        let eid = Self::intern_id(ids, next_id, e);
        let label = Self::var_label(var, labels);
        writeln!(out, "  n{} [shape=ellipse,label=\"{}\"];", this, label)?;
        writeln!(out, "  n{} -> n{} [label=\"1\"];", this, tid)?;
        writeln!(out, "  n{} -> n{} [label=\"0\",style=dashed];", this, eid)?;
        self.dump_add_dot_rec(t, out, labels, ids, next_id, visited)?;
        self.dump_add_dot_rec(e, out, labels, ids, next_id, visited)?;
        Ok(())
    }

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
            let tid = Self::intern_id(ids, next_id, t.regular());
            format!("n{}", tid)
        };
        let e_target = if e.is_one() {
            "ONE".to_string()
        } else if e.is_zero() {
            "ZERO".to_string()
        } else {
            let eid = Self::intern_id(ids, next_id, e.regular());
            format!("n{}", eid)
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

    /// Given a vector of variables (x0, x_1, ...),
    /// return the ADD that maps (x0, x1, ...) value
    /// assuming x0 is the LSB, x1 is the next bit, etc.\
    /// __Refs__: result \
    /// __Derefs__: None
    pub fn get_encoding(&mut self, nodes: &Vec<NodeId>) -> NodeId {
        let mut result = self.add_const(0.0);

        for bm in 0..(1i32 << nodes.len()) {
            let mut term = self.one();
            for i in 0..nodes.len() {
                let var = nodes[i];
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

        result
    }
}
