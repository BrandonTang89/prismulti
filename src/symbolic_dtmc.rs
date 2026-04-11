use std::collections::HashMap;

use lumindd::NodeId;

use crate::analyze::DTMCModelInfo;
use crate::ast::DTMCAst;
use crate::reachability::count_transitions_minterms;
use crate::ref_manager::{RefManager, LEAK_REPORT_LIMIT};

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
    pub var_curr_nodes: HashMap<String, Vec<NodeId>>,
    /// Variable name -> next-state DD bit nodes (LSB..MSB).
    pub var_next_nodes: HashMap<String, Vec<NodeId>>,

    /// DD node -> human-friendly name used in DOT output.
    pub dd_var_names: HashMap<NodeId, String>,

    /// BDD cube over all next-state variables.
    pub next_var_cube: NodeId,
    /// BDD cube over all current-state variables.
    pub curr_var_cube: NodeId,

    /// MTBDD transition relation P(s,s').
    pub transitions: NodeId,
    /// 0-1 BDD support of filtered transitions.
    pub transitions_01_bdd: NodeId,
}

impl SymbolicDTMC {
    /// Create an empty symbolic DTMC and allocate base roots.
    pub fn new(ast: DTMCAst, info: DTMCModelInfo) -> Self {
        let mut mgr = RefManager::new();
        let transitions = mgr.add_zero();
        let transitions_01_bdd = mgr.bdd_zero();
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
            transitions_01_bdd,
        }
    }

    /// Number of state bits in the current/next encoding.
    pub fn state_bit_counts(&self) -> (usize, usize) {
        let curr = self.var_curr_nodes.values().map(|v| v.len()).sum();
        let next = self.var_next_nodes.values().map(|v| v.len()).sum();
        (curr, next)
    }

    /// Human-readable summary of transition relation statistics.
    pub fn describe(&mut self) -> String {
        let mut desc = String::new();
        desc.push_str("Variables:\n");
        for (var_name, curr_nodes) in &self.var_curr_nodes {
            let next_nodes = &self.var_next_nodes[var_name];
            desc.push_str(&format!(
                "  {}: curr nodes {:?}, next nodes {:?}\n",
                var_name, curr_nodes, next_nodes
            ));
        }

        desc.push_str(&format!(
            "Transitions ADD node ID: {:?}\n",
            self.transitions
        ));
        desc.push_str(&format!(
            "Transitions 0-1 BDD node ID: {:?}\n",
            self.transitions_01_bdd
        ));

        let (curr_bits, next_bits) = self.state_bit_counts();
        let stats = self
            .mgr
            .add_stats(self.transitions, (curr_bits + next_bits) as u32);
        let transitions = count_transitions_minterms(self);
        desc.push_str(&format!(
            "Num Nodes ADD: {}, Num Terminals: {}, ADD non-zero minterms: {}, Transitions(minterms): {}\n",
            stats.node_count, stats.terminal_count, stats.minterms, transitions
        ));
        desc
    }
}

impl Drop for SymbolicDTMC {
    fn drop(&mut self) {
        self.mgr.deref_node(self.transitions);
        self.mgr.deref_node(self.transitions_01_bdd);
        self.mgr.deref_node(self.curr_var_cube);
        self.mgr.deref_node(self.next_var_cube);

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

        let leaks = self.mgr.nonzero_ref_count();
        println!("RefManager non-zero refs before drop: {}", leaks);
        if leaks > 0 {
            for (node, count) in self.mgr.nonzero_ref_entries(LEAK_REPORT_LIMIT) {
                println!("  {:?} -> {}", node, count);
            }
        }

        // Drop framework roots we intentionally keep alive while the manager runs.
        self.mgr.deref_node(NodeId::ONE);
        self.mgr.deref_node(NodeId::ZERO);
        self.mgr.deref_node(NodeId::ZERO);

        let leaks_after = self.mgr.nonzero_ref_count();
        println!("RefManager non-zero refs after constants: {}", leaks_after);
        if leaks_after > 0 {
            for (node, count) in self.mgr.nonzero_ref_entries(LEAK_REPORT_LIMIT) {
                println!("  {:?} -> {}", node, count);
            }
        }
    }
}
