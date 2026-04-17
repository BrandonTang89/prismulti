use std::cell::OnceCell;
use std::collections::HashMap;

use tracing::info;

use crate::analyze::DTMCModelInfo;
use crate::ast::DTMCAst;
use crate::ast::utils::init_value;
use crate::dd_manager::dd;
use crate::dd_manager::protected_local::{ProtectedAddLocal, ProtectedBddLocal};
use crate::dd_manager::protected_slot::{ProtectedAddSlot, ProtectedBddSlot, ProtectedVarSetSlot};
use crate::dd_manager::{BDDVAR, BddNode, DDManager};

/// Symbolic DTMC representation used by construction and analysis passes.
pub struct SymbolicDTMC {
    /// Decision diagram manager with reference-tracking wrappers.
    pub mgr: DDManager,

    /// Owned model AST.
    pub ast: DTMCAst,

    /// Owned model analysis information.
    pub info: DTMCModelInfo,

    /// Variable name -> current-state DD bit nodes (LSB..MSB).
    pub curr_name_to_indices: HashMap<String, Vec<BDDVAR>>,
    /// Variable name -> next-state DD bit nodes (LSB..MSB).
    pub next_name_to_indices: HashMap<String, Vec<BDDVAR>>,

    /// Protected roots for state-variable DD nodes stored in the maps above.
    pub var_node_roots: Vec<ProtectedBddSlot>,

    /// Current-state variable indices aligned with `next_var_indices`.
    pub curr_var_indices: Vec<BDDVAR>,
    /// Next-state variable indices aligned with `curr_var_indices`.
    pub next_var_indices: Vec<BDDVAR>,

    /// DD node -> human-friendly name used in DOT output.
    pub dd_var_names: HashMap<BDDVAR, String>,

    /// 0-1 ADD cube over all next-state variables.
    pub next_var_cube: ProtectedVarSetSlot,
    /// 0-1 ADD cube over all current-state variables.
    pub curr_var_set: ProtectedVarSetSlot,

    /// ADD transition relation P(s,s').
    pub transitions: ProtectedAddSlot,

    // == Values derived after construction ==
    /// 0-1 ADD support of filtered transitions.
    transitions_01: OnceCell<ProtectedBddSlot>,

    /// Initial state over current-state variables as a 0-1 BDD.
    init: OnceCell<ProtectedBddSlot>,

    /// Cached BDD for `(curr == next)` over all state bits.
    curr_next_identity: OnceCell<ProtectedBddSlot>,

    /// Reachable states over current-state variables as a 0-1 BDD.
    reachable: OnceCell<ProtectedBddSlot>,
}

impl SymbolicDTMC {
    /// Create an empty symbolic DTMC and allocate base roots.
    pub fn new(ast: DTMCAst, info: DTMCModelInfo) -> Self {
        let mgr = DDManager::new();
        let transitions = ProtectedAddSlot::new(dd::add_zero(&mgr));
        let next_var_cube = ProtectedVarSetSlot::new(dd::var_set_empty(&mgr));
        let curr_var_cube = ProtectedVarSetSlot::new(dd::var_set_empty(&mgr));

        Self {
            mgr,
            ast,
            info,
            curr_name_to_indices: HashMap::new(),
            next_name_to_indices: HashMap::new(),
            var_node_roots: Vec::new(),
            curr_var_indices: Vec::new(),
            next_var_indices: Vec::new(),
            dd_var_names: HashMap::new(),
            next_var_cube,
            curr_var_set: curr_var_cube,
            transitions,
            transitions_01: OnceCell::new(),
            init: OnceCell::new(),
            reachable: OnceCell::new(),
            curr_next_identity: OnceCell::new(),
        }
    }

    /// Number of state variables in the current/next encoding.
    pub fn state_variable_counts(&self) -> (u32, u32) {
        let curr = self
            .curr_name_to_indices
            .values()
            .map(|v| v.len() as u32)
            .sum();
        let next = self
            .next_name_to_indices
            .values()
            .map(|v| v.len() as u32)
            .sum();
        (curr, next)
    }

    /// Total number of variables used
    pub fn total_variable_count(&self) -> u32 {
        self.state_variable_counts().0 + self.state_variable_counts().1
    }

    /// Number of reachable states in the DTMC
    pub fn reachable_state_count(&mut self) -> u64 {
        dd::bdd_count_minterms(
            &self.mgr,
            self.reachable
                .get()
                .map(ProtectedBddSlot::get)
                .expect("Reachable states should be computed by now"),
            self.curr_var_indices.len() as u32,
        )
    }

    /// Human-readable summary of transition relation statistics.
    pub fn describe(&mut self) -> Vec<String> {
        let mut desc = Vec::new();
        desc.push("Variables:\n".into());
        for (var_name, curr_nodes) in &self.curr_name_to_indices {
            let next_nodes = &self.next_name_to_indices[var_name];
            desc.push(format!(
                "  {}: curr nodes {:?}, next nodes {:?}\n",
                var_name, curr_nodes, next_nodes
            ));
        }

        desc.push(format!(
            "Transitions ADD node ID: {:?}\n",
            self.transitions.get()
        ));
        desc.push(format!(
            "Transitions 0-1 ADD node ID: {:?}\n",
            self.transitions_01.get().map(ProtectedBddSlot::get)
        ));

        let (curr_bits, next_bits) = self.state_variable_counts();
        let stats = dd::add_stats(&self.mgr, self.transitions.get(), curr_bits + next_bits);
        desc.push(format!(
            "Num Nodes ADD: {}, Num Terminals: {}, Transitions(minterms): {}\n",
            stats.node_count, stats.terminal_count, stats.minterms
        ));
        desc
    }

    fn build_identity_transition_bdd(&mut self) -> BddNode {
        let mut ident = ProtectedBddLocal::new(dd::bdd_one(&mut self.mgr));
        for (&curr_idx, &next_idx) in self
            .curr_var_indices
            .iter()
            .zip(self.next_var_indices.iter())
        {
            let curr = ProtectedBddLocal::new(dd::bdd_var(&self.mgr, curr_idx));
            let next = ProtectedBddLocal::new(dd::bdd_var(&self.mgr, next_idx));
            let eq = ProtectedBddLocal::new(dd::bdd_equals(&self.mgr, curr.get(), next.get()));
            ident.set(dd::bdd_and(&self.mgr, ident.get(), eq.get()));
        }
        ident.get()
    }

    /// __Refs__: result \
    /// __Derefs__: None
    pub fn get_curr_next_identity_bdd(&mut self) -> BddNode {
        if let Some(identity) = self.curr_next_identity.get() {
            return identity.get();
        }

        let identity = self.build_identity_transition_bdd();
        self.curr_next_identity
            .set(ProtectedBddSlot::new(identity))
            .expect("Current/next identity BDD should only be set once");
        identity
    }

    /// Builds the initial-state BDD over current-state variables.
    ///
    /// Analysis already guarantees folded literal inits and in-range values.
    /// The assertions below therefore check internal consistency only.
    fn build_init_bdd(&mut self) -> BddNode {
        let mut init = ProtectedBddLocal::new(dd::bdd_one(&mut self.mgr));

        for module in &self.ast.modules {
            for var_decl in &module.local_vars {
                let var_name = var_decl.name.clone();
                let (lo, hi) = self.info.var_bounds[&var_name];
                let init_val = init_value(var_decl);
                assert!(init_val >= lo && init_val <= hi);

                let encoded = (init_val - lo) as u32;
                let curr_nodes = self.curr_name_to_indices[&var_name].clone();
                for (i, var_idx) in curr_nodes.into_iter().enumerate() {
                    let lit = ProtectedBddLocal::new(if (encoded & (1u32 << i)) != 0 {
                        dd::bdd_var(&self.mgr, var_idx)
                    } else {
                        let var = ProtectedBddLocal::new(dd::bdd_var(&self.mgr, var_idx));
                        dd::bdd_not(&self.mgr, var.get())
                    });
                    init.set(dd::bdd_and(&self.mgr, init.get(), lit.get()));
                }
            }
        }

        debug_assert_eq!(
            dd::bdd_count_minterms(&self.mgr, init.get(), self.curr_var_indices.len() as u32),
            1
        );

        init.get()
    }

    /// __Refs__: result\
    /// __Derefs__: None
    pub fn get_init_bdd(&mut self) -> BddNode {
        if let Some(init) = self.init.get() {
            return init.get();
        }

        let init = self.build_init_bdd();
        self.init
            .set(ProtectedBddSlot::new(init))
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
            .set(ProtectedBddSlot::new(reachable))
            .expect("Reachable states should only be set once");

        // Filter the transition relation
        let reachable_add = ProtectedAddLocal::new(dd::bdd_to_add(&mut self.mgr, reachable));
        let old_transitions = self.transitions.get();
        self.transitions.set(dd::add_times(
            &mut self.mgr,
            old_transitions,
            reachable_add.get(),
        ));

        // Filter the 0-1 transition relation
        let filtered_01 =
            ProtectedBddLocal::new(dd::add_to_bdd(&mut self.mgr, self.transitions.get()));

        // Add self-loops to dead-end states
        let out_curr = ProtectedBddLocal::new(dd::bdd_exists_abstract(
            &self.mgr,
            filtered_01.get(),
            self.next_var_cube.get(),
        ));

        let not_out_curr = ProtectedBddLocal::new(dd::bdd_not(&self.mgr, out_curr.get()));

        let dead_end_curr =
            ProtectedBddLocal::new(dd::bdd_and(&self.mgr, reachable, not_out_curr.get()));

        let dead_end_count = dd::bdd_count_minterms(
            &self.mgr,
            dead_end_curr.get(),
            self.curr_var_indices.len() as u32,
        );

        if dead_end_count > 0 {
            let curr_next_eq = self.get_curr_next_identity_bdd();
            let self_loops =
                ProtectedBddLocal::new(dd::bdd_and(&self.mgr, dead_end_curr.get(), curr_next_eq));

            // Set transitions_01 to include self-loops on dead-end states
            self.transitions_01
                .set(ProtectedBddSlot::new(dd::bdd_or(
                    &self.mgr,
                    filtered_01.get(),
                    self_loops.get(),
                )))
                .expect("Transitions 0-1 should only be set once");

            // Set transitions to include self-loops on dead-end states
            let self_loops_add =
                ProtectedAddLocal::new(dd::bdd_to_add(&mut self.mgr, self_loops.get()));
            let original_trans = self.transitions.get();
            self.transitions.set(dd::add_plus(
                &mut self.mgr,
                original_trans,
                self_loops_add.get(),
            ));
        } else {
            self.transitions_01
                .set(ProtectedBddSlot::new(filtered_01.get()))
                .expect("Transitions 0-1 should only be set once");
        }

        info!("Added self-loops to {} dead-end states", dead_end_count);
    }

    /// __Refs__: result\
    /// __Derefs__: None
    pub fn get_reachable_bdd(&mut self) -> BddNode {
        self.reachable
            .get()
            .map(ProtectedBddSlot::get)
            .expect("Reachable states should be computed by now")
    }

    /// __Refs__: result\
    /// __Derefs__: None
    pub fn get_transitions_01(&mut self) -> BddNode {
        self.transitions_01
            .get()
            .map(ProtectedBddSlot::get)
            .expect("Transitions 0-1 should be set based on reachable states")
    }
}
