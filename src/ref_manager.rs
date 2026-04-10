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

    /// Given a vector of variables (x0, x_1, ...),
    /// return the ADD that maps (x0, x1, ...) value
    /// assuming x0 is the LSB, x1 is the next bit, etc.\
    /// __Refs__: result \
    /// __Derefs__: None
    pub fn get_encoding(&mut self, nodes: &Vec<NodeId>) -> NodeId {
        let mut result = self.zero();

        for bm in 0..(1i32 << nodes.len()) {
            let mut term = self.zero();
            for i in 0..nodes.len() {
                let var = nodes[i];
                self.ref_node(var);
                let literal = if (bm & (1 << i)) == 1 {
                    var
                } else {
                    self.bdd_not(var)
                };
                term = self.bdd_and(term, literal);
            }
            let value = self.add_const(bm as f64);
            let term = self.add_times(term, value);
            result = self.add_times(result, term);
        }

        result
    }
}
