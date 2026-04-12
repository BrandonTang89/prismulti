use std::collections::HashMap;

use tracing::error;

use crate::analyze::DTMCModelInfo;
use crate::ast::DTMCAst;
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

    /// DD node -> human-friendly name used in DOT output.
    pub dd_var_names: HashMap<Node, String>,

    /// 0-1 ADD cube over all next-state variables.
    pub next_var_cube: BddNode,
    /// 0-1 ADD cube over all current-state variables.
    pub curr_var_cube: BddNode,

    /// ADD transition relation P(s,s').
    pub transitions: AddNode,
    /// 0-1 ADD support of filtered transitions.
    pub transitions_01_add: BddNode,

    /// Number of reachable states computed by BFS during construction.
    pub reachable_states: u64,

    released: bool,
}

impl SymbolicDTMC {
    /// Create an empty symbolic DTMC and allocate base roots.
    pub fn new(ast: DTMCAst, info: DTMCModelInfo) -> Self {
        let mut mgr = RefManager::new();
        let transitions = mgr.add_zero();
        let transitions_01_add = mgr.bdd_zero();
        let next_var_cube = mgr.bdd_one();
        let curr_var_cube = mgr.bdd_one();

        Self {
            mgr,
            ast,
            info,
            var_curr_nodes: HashMap::new(),
            var_next_nodes: HashMap::new(),
            dd_var_names: HashMap::new(),
            next_var_cube,
            curr_var_cube,
            transitions,
            transitions_01_add,
            reachable_states: 0,
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
        self.reachable_states
    }

    fn release_refs(&mut self) -> RefLeakReport {
        if self.released {
            return RefLeakReport {
                nonzero_ref_count: 0,
            };
        }

        self.mgr.deref_node(self.transitions.0);
        self.mgr.deref_node(self.transitions_01_add.0);
        self.mgr.deref_node(self.curr_var_cube.0);
        self.mgr.deref_node(self.next_var_cube.0);

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
            self.transitions_01_add
        ));

        let (curr_bits, next_bits) = self.state_variable_counts();
        let stats = self
            .mgr
            .add_stats(self.transitions, (curr_bits + next_bits) as u32);
        desc.push(format!(
            "Num Nodes ADD: {}, Num Terminals: {}, Transitions(minterms): {}\n",
            stats.node_count, stats.terminal_count, stats.minterms
        ));
        desc
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
