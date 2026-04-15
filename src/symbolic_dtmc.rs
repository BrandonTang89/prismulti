use std::cell::OnceCell;
use std::collections::HashMap;

use tracing::error;

use crate::analyze::DTMCModelInfo;
use crate::ast::DTMCAst;
use crate::ast::utils::init_value;
use crate::ref_manager::{AddNode, BddNode, Node, RefManager};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLeakReport {
    pub nonzero_ref_count: usize,
}

/// Symbolic DTMC representation used by construction and analysis passes.
///
/// The DD manager and all DD roots are owned here so the structure can cleanly
/// release references on drop.
pub struct SymbolicDTMC {
    /// Decision diagram manager with reference-tracking wrappers.
    pub mgr: RefManager,

    /// Owned model AST.
    pub ast: DTMCAst,

    /// Owned model analysis information.
    pub info: DTMCModelInfo,

    /// Variable name -> current-state DD bit nodes (LSB..MSB).
    pub var_curr_nodes: HashMap<String, Vec<Node>>,
    /// Variable name -> next-state DD bit nodes (LSB..MSB).
    pub var_next_nodes: HashMap<String, Vec<Node>>,

    /// Current-state variable indices aligned with `next_var_indices`.
    pub curr_var_indices: Vec<u16>,
    /// Next-state variable indices aligned with `curr_var_indices`.
    pub next_var_indices: Vec<u16>,

    /// DD node -> human-friendly name used in DOT output.
    pub dd_var_names: HashMap<Node, String>,

    /// 0-1 ADD cube over all next-state variables.
    pub next_var_cube: BddNode,
    /// 0-1 ADD cube over all current-state variables.
    pub curr_var_cube: BddNode,

    /// ADD transition relation P(s,s').
    pub transitions: AddNode,

    // == Values derived after construction ==
    /// 0-1 ADD support of filtered transitions.
    transitions_01: OnceCell<BddNode>,

    /// Initial state over current-state variables as a 0-1 BDD.
    init: OnceCell<BddNode>,

    /// Cached BDD for `(curr == next)` over all state bits.
    curr_next_identity: OnceCell<BddNode>,

    /// Reachable states over current-state variables as a 0-1 BDD.
    reachable: OnceCell<BddNode>,

    /// Whether all the DD roots have been released.
    /// Set by `release_refs`
    released: bool,
}

impl SymbolicDTMC {
    /// Create an empty symbolic DTMC and allocate base roots.
    pub fn new(ast: DTMCAst, info: DTMCModelInfo) -> Self {
        let mut mgr = RefManager::new();
        let transitions = mgr.add_zero();
        let next_var_cube = mgr.bdd_one();
        let curr_var_cube = mgr.bdd_one();

        Self {
            mgr,
            ast,
            info,
            var_curr_nodes: HashMap::new(),
            var_next_nodes: HashMap::new(),
            curr_var_indices: Vec::new(),
            next_var_indices: Vec::new(),
            dd_var_names: HashMap::new(),
            next_var_cube,
            curr_var_cube,
            transitions,
            transitions_01: OnceCell::new(),
            init: OnceCell::new(),
            reachable: OnceCell::new(),
            curr_next_identity: OnceCell::new(),
            released: false,
        }
    }

    /// Number of state variables in the current/next encoding.
    pub fn state_variable_counts(&self) -> (u32, u32) {
        let curr = self.var_curr_nodes.values().map(|v| v.len() as u32).sum();
        let next = self.var_next_nodes.values().map(|v| v.len() as u32).sum();
        (curr, next)
    }

    /// Total number of variables used
    pub fn total_variable_count(&self) -> u32 {
        self.state_variable_counts().0 + self.state_variable_counts().1
    }

    /// Number of reachable states in the DTMC
    pub fn reachable_state_count(&mut self) -> u64 {
        self.mgr.bdd_count_minterms(
            self.reachable
                .get()
                .cloned()
                .expect("Reachable states should be computed by now"),
            self.curr_var_indices.len() as u32,
        )
    }

    fn release_refs(&mut self) -> RefLeakReport {
        if self.released {
            return RefLeakReport {
                nonzero_ref_count: 0,
            };
        }

        self.mgr.deref_node(self.transitions.0);
        self.mgr.deref_node(self.curr_var_cube.0);
        self.mgr.deref_node(self.next_var_cube.0);

        if let Some(init) = self.init.take() {
            self.mgr.deref_node(init.0);
        }
        if let Some(reachable) = self.reachable.take() {
            self.mgr.deref_node(reachable.0);
        }
        if let Some(trans_01) = self.transitions_01.take() {
            self.mgr.deref_node(trans_01.0);
        }
        if let Some(identity) = self.curr_next_identity.take() {
            self.mgr.deref_node(identity.0);
        }

        for nodes in self.var_curr_nodes.values() {
            for &node in nodes {
                self.mgr.deref_node(node);
            }
        }
        for nodes in self.var_next_nodes.values() {
            for &node in nodes {
                self.mgr.deref_node(node);
            }
        }

        self.released = true;
        RefLeakReport {
            nonzero_ref_count: self.mgr.nonzero_ref_count(),
        }
    }

    pub fn release_report(&mut self) -> RefLeakReport {
        self.release_refs()
    }

    /// Human-readable summary of transition relation statistics.
    pub fn describe(&mut self) -> Vec<String> {
        let mut desc = Vec::new();
        desc.push("Variables:\n".into());
        for (var_name, curr_nodes) in &self.var_curr_nodes {
            let next_nodes = &self.var_next_nodes[var_name];
            desc.push(format!(
                "  {}: curr nodes {:?}, next nodes {:?}\n",
                var_name, curr_nodes, next_nodes
            ));
        }

        desc.push(format!("Transitions ADD node ID: {:?}\n", self.transitions));
        desc.push(format!(
            "Transitions 0-1 ADD node ID: {:?}\n",
            self.transitions_01.get()
        ));

        let (curr_bits, next_bits) = self.state_variable_counts();
        let stats = self.mgr.add_stats(self.transitions, curr_bits + next_bits);
        desc.push(format!(
            "Num Nodes ADD: {}, Num Terminals: {}, Transitions(minterms): {}\n",
            stats.node_count, stats.terminal_count, stats.minterms
        ));
        desc
    }

    fn build_identity_transition_bdd(&mut self) -> BddNode {
        let mut ident = self.mgr.bdd_one();
        for (&curr_idx, &next_idx) in self
            .curr_var_indices
            .iter()
            .zip(self.next_var_indices.iter())
        {
            let curr = self.mgr.bdd_var(curr_idx);
            let next = self.mgr.bdd_var(next_idx);
            let eq = self.mgr.bdd_equals(curr, next);
            ident = self.mgr.bdd_and(ident, eq);
        }
        ident
    }

    /// __Refs__: result \
    /// __Derefs__: None
    pub fn get_curr_next_identity_bdd(&mut self) -> BddNode {
        if let Some(identity) = self.curr_next_identity.get().cloned() {
            self.mgr.ref_node(identity.0);
            return identity;
        }

        let identity = self.build_identity_transition_bdd();
        self.mgr.ref_node(identity.0);
        self.curr_next_identity
            .set(identity)
            .expect("Current/next identity BDD should only be set once");
        identity
    }

    /// Builds the initial-state BDD over current-state variables.
    ///
    /// Analysis already guarantees folded literal inits and in-range values.
    /// The assertions below therefore check internal consistency only.
    fn build_init_bdd(&mut self) -> BddNode {
        let mut init = self.mgr.bdd_one();

        for module in &self.ast.modules {
            for var_decl in &module.local_vars {
                let var_name = var_decl.name.clone();
                let (lo, hi) = self.info.var_bounds[&var_name];
                let init_val = init_value(var_decl);
                assert!(init_val >= lo && init_val <= hi);

                let encoded = (init_val - lo) as u32;
                let curr_nodes = self.var_curr_nodes[&var_name].clone();
                for (i, bit) in curr_nodes.into_iter().enumerate() {
                    self.mgr.ref_node(bit);
                    let lit = if (encoded & (1u32 << i)) != 0 {
                        BddNode(bit)
                    } else {
                        self.mgr.bdd_not(BddNode(bit))
                    };
                    init = self.mgr.bdd_and(init, lit);
                }
            }
        }

        debug_assert_eq!(
            self.mgr
                .bdd_count_minterms(init, self.curr_var_indices.len() as u32),
            1
        );

        init
    }

    /// __Refs__: result\
    /// __Derefs__: None
    pub fn get_init_bdd(&mut self) -> BddNode {
        if let Some(init) = self.init.get().cloned() {
            self.mgr.ref_node(init.0);
            return init;
        }

        let init = self.build_init_bdd();
        self.mgr.ref_node(init.0);
        self.init
            .set(init)
            .expect("Initial-state BDD should only be set once");
        init
    }

    /// Takes ownership of the reachable states BDD \
    /// Also sets transitions_01 based on reachable states
    /// Filters out unreachable states
    pub fn set_reachable_and_filter(&mut self, reachable: BddNode) {
        assert!(
            self.reachable.get().is_none(),
            "Reachable states should only be set once"
        );
        assert!(
            self.transitions_01.get().is_none(),
            "Transitions 0-1 should be set based on reachable states"
        );
        self.reachable
            .set(reachable)
            .expect("Reachable states should only be set once");

        // Filter the transition relation
        self.mgr.ref_node(reachable.0);
        let reachable_add = self.mgr.bdd_to_add(reachable);
        let old_transitions = self.transitions;
        self.transitions = self.mgr.add_times(old_transitions, reachable_add);

        // Filter the 0-1 transition relation
        self.mgr.ref_node(self.transitions.0);
        let filtered_01 = self.mgr.add_to_bdd(self.transitions);

        // Add self-loops to dead-end states
        self.mgr.ref_node(filtered_01.0);
        let out_curr = self
            .mgr
            .bdd_exists_abstract(filtered_01, self.next_var_cube);

        let not_out_curr = self.mgr.bdd_not(out_curr);

        self.mgr.ref_node(reachable.0);
        let dead_end_curr = self.mgr.bdd_and(reachable, not_out_curr);

        let dead_end_count = self
            .mgr
            .bdd_count_minterms(dead_end_curr, self.curr_var_indices.len() as u32);

        if dead_end_count > 0 {
            let curr_next_eq = self.get_curr_next_identity_bdd();
            self.mgr.ref_node(dead_end_curr.0);
            let self_loops = self.mgr.bdd_and(dead_end_curr, curr_next_eq);

            // Set transitions_01 to include self-loops on dead-end states
            // consume filtered_01
            self.mgr.ref_node(self_loops.0);
            self.transitions_01
                .set(self.mgr.bdd_or(filtered_01, self_loops))
                .expect("Transitions 0-1 should only be set once");

            // Set transitions to include self-loops on dead-end states
            let self_loops_add = self.mgr.bdd_to_add(self_loops);
            let original_trans = self.transitions;
            self.transitions = self.mgr.add_plus(original_trans, self_loops_add);
        } else {
            self.transitions_01
                .set(filtered_01)
                .expect("Transitions 0-1 should only be set once"); // own filtered_01
        }

        println!("Added self-loops to {} dead-end states", dead_end_count);
        self.mgr.deref_node(dead_end_curr.0);
    }

    /// __Refs__: result\
    /// __Derefs__: None
    pub fn get_reachable_bdd(&mut self) -> BddNode {
        let reachable = self
            .reachable
            .get()
            .cloned()
            .expect("Reachable states should be computed by now");
        self.mgr.ref_node(reachable.0);
        reachable
    }

    /// __Refs__: result\
    /// __Derefs__: None
    pub fn get_transitions_01(&mut self) -> BddNode {
        let trans_01 = self
            .transitions_01
            .get()
            .cloned()
            .expect("Transitions 0-1 should be set based on reachable states");
        self.mgr.ref_node(trans_01.0);
        trans_01
    }
}

impl Drop for SymbolicDTMC {
    fn drop(&mut self) {
        let report = self.release_refs();
        if report.nonzero_ref_count > 0 {
            error!(
                "RefManager non-zero refs after owned release: {}",
                report.nonzero_ref_count
            );
        }
    }
}
