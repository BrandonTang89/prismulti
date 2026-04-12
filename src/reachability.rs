use crate::ast::{Expr, VarDecl, VarType};
use crate::symbolic_dtmc::SymbolicDTMC;

/// Extract a concrete integer from an initial-value expression.
fn init_value(var_decl: &VarDecl) -> i32 {
    match (&var_decl.var_type, &*var_decl.init) {
        (VarType::BoundedInt { .. }, Expr::IntLit(v)) => *v,
        (VarType::Bool, Expr::BoolLit(b)) => {
            if *b {
                1
            } else {
                0
            }
        }
        (VarType::Bool, Expr::IntLit(v)) if *v == 0 || *v == 1 => *v,
        _ => panic!(
            "Unsupported init expression for variable '{}': {:?}",
            var_decl.name, var_decl.init
        ),
    }
}

/// Build BDD for the unique initial state over current-state bits.
fn build_init_bdd(dtmc: &mut SymbolicDTMC) -> lumindd::NodeId {
    let mut init = dtmc.mgr.bdd_one();

    for module in &dtmc.ast.modules {
        for var_decl in &module.local_vars {
            let var_name = var_decl.name.clone();
            let (lo, hi) = dtmc.info.var_bounds[&var_name];
            let init_val = init_value(var_decl);
            assert!(
                init_val >= lo && init_val <= hi,
                "Initial value of '{}' out of bounds: {} not in [{}..{}]",
                var_name,
                init_val,
                lo,
                hi
            );

            let encoded = (init_val - lo) as u32;
            let curr_nodes = dtmc.var_curr_nodes[&var_name].clone();
            for (i, bit) in curr_nodes.into_iter().enumerate() {
                dtmc.mgr.ref_node(bit);
                let lit = if (encoded & (1u32 << i)) != 0 {
                    bit
                } else {
                    dtmc.mgr.bdd_not(bit)
                };
                init = dtmc.mgr.bdd_and(init, lit);
            }
        }
    }

    init
}

fn curr_next_var_indices(dtmc: &SymbolicDTMC) -> (Vec<u16>, Vec<u16>) {
    let mut curr_indices = Vec::new();
    let mut next_indices = Vec::new();

    for module in &dtmc.ast.modules {
        for var_decl in &module.local_vars {
            let var_name = &var_decl.name;
            let curr_nodes = &dtmc.var_curr_nodes[var_name];
            let next_nodes = &dtmc.var_next_nodes[var_name];
            for (&curr, &next) in curr_nodes.iter().zip(next_nodes.iter()) {
                curr_indices.push(dtmc.mgr.read_var_index(curr.regular()));
                next_indices.push(dtmc.mgr.read_var_index(next.regular()));
            }
        }
    }

    (curr_indices, next_indices)
}

fn build_curr_next_identity_bdd(dtmc: &mut SymbolicDTMC) -> lumindd::NodeId {
    let mut ident = dtmc.mgr.bdd_one();
    for module in &dtmc.ast.modules {
        for var_decl in &module.local_vars {
            let var_name = &var_decl.name;
            let curr_nodes = dtmc.var_curr_nodes[var_name].clone();
            let next_nodes = dtmc.var_next_nodes[var_name].clone();
            for (curr, next) in curr_nodes.into_iter().zip(next_nodes.into_iter()) {
                dtmc.mgr.ref_node(curr);
                dtmc.mgr.ref_node(next);
                let eq = dtmc.mgr.bdd_equals(curr, next);
                ident = dtmc.mgr.bdd_and(ident, eq);
            }
        }
    }
    ident
}

fn add_dead_end_self_loops(dtmc: &mut SymbolicDTMC, reachable: lumindd::NodeId) {
    dtmc.mgr.ref_node(dtmc.transitions_01_bdd);
    let out_curr = dtmc
        .mgr
        .bdd_or_abstract(dtmc.transitions_01_bdd, dtmc.next_var_cube);

    dtmc.mgr.ref_node(out_curr);
    let not_out_curr = dtmc.mgr.bdd_not(out_curr);

    dtmc.mgr.ref_node(reachable);
    let dead_end_curr = dtmc.mgr.bdd_and(reachable, not_out_curr);
    dtmc.mgr.deref_node(out_curr);

    dtmc.mgr.ref_node(dead_end_curr);
    let dead_end_add_for_count = dtmc.mgr.bdd_to_add(dead_end_curr);
    let dead_end_count_add = dtmc
        .mgr
        .add_sum_abstract(dead_end_add_for_count, dtmc.curr_var_cube);
    let dead_end_count = dtmc
        .mgr
        .add_value(dead_end_count_add.regular())
        .unwrap_or(0.0)
        .round() as u64;
    dtmc.mgr.deref_node(dead_end_count_add);

    if dead_end_count > 0 {
        let curr_next_eq = build_curr_next_identity_bdd(dtmc);
        dtmc.mgr.ref_node(dead_end_curr);
        let self_loops_bdd = dtmc.mgr.bdd_and(dead_end_curr, curr_next_eq);

        let old_bdd = dtmc.transitions_01_bdd;
        dtmc.mgr.ref_node(old_bdd);
        dtmc.mgr.ref_node(self_loops_bdd);
        dtmc.transitions_01_bdd = dtmc.mgr.bdd_or(old_bdd, self_loops_bdd);
        dtmc.mgr.deref_node(old_bdd);

        dtmc.mgr.ref_node(self_loops_bdd);
        let self_loops_add = dtmc.mgr.bdd_to_add(self_loops_bdd);
        let old_add = dtmc.transitions;
        dtmc.mgr.ref_node(old_add);
        dtmc.transitions = dtmc.mgr.add_plus(old_add, self_loops_add);
        dtmc.mgr.deref_node(old_add);
        dtmc.mgr.deref_node(self_loops_bdd);
    }

    println!("Added self-loops to {} dead-end states", dead_end_count);
    dtmc.mgr.deref_node(dead_end_curr);
}

/// Compute least fixed-point reachability and filter transition relation.
///
/// Steps:
/// 1. Convert transition ADD to 0-1 BDD.
/// 2. Iterate `R := R OR post(R)` using BDD image computation.
/// 3. Restrict transitions to reachable current states.
/// 4. Store filtered 0-1 transition BDD in `dtmc.transitions_01_bdd`.
pub fn compute_reachable_and_filter(dtmc: &mut SymbolicDTMC) {
    let mut reachable = build_init_bdd(dtmc);

    dtmc.mgr.ref_node(dtmc.transitions);
    let trans_bdd = dtmc.mgr.add_to_bdd(dtmc.transitions);

    let (curr_indices, next_indices) = curr_next_var_indices(dtmc);
    let mut iterations = 0usize;

    loop {
        iterations += 1;
        let old = reachable;

        dtmc.mgr.ref_node(old);
        dtmc.mgr.ref_node(trans_bdd);
        let image_next = dtmc
            .mgr
            .bdd_and_abstract(old, trans_bdd, dtmc.curr_var_cube);
        let image_curr = dtmc
            .mgr
            .bdd_swap_variables(image_next, &next_indices, &curr_indices);
        let new_reachable = dtmc.mgr.bdd_or(old, image_curr);

        reachable = new_reachable;
        if new_reachable == old {
            break;
        }
    }

    dtmc.mgr.ref_node(reachable);
    let reachable_add_for_count = dtmc.mgr.bdd_to_add(reachable);
    let reachable_count_add = dtmc
        .mgr
        .add_sum_abstract(reachable_add_for_count, dtmc.curr_var_cube);
    let reachable_states = dtmc
        .mgr
        .add_value(reachable_count_add.regular())
        .unwrap_or(0.0)
        .round() as u64;
    dtmc.reachable_states = reachable_states;
    dtmc.mgr.deref_node(reachable_count_add);

    println!(
        "Reachability (BFS): {} iterations, reachable states: {}",
        iterations, reachable_states
    );

    dtmc.mgr.ref_node(reachable);
    let reachable_add = dtmc.mgr.bdd_to_add(reachable);
    dtmc.mgr.deref_node(dtmc.transitions);
    dtmc.transitions = dtmc.mgr.add_times(dtmc.transitions, reachable_add);

    dtmc.mgr.ref_node(dtmc.transitions);
    let filtered_01_bdd = dtmc.mgr.add_to_bdd(dtmc.transitions);
    let old_bdd = dtmc.transitions_01_bdd;
    dtmc.transitions_01_bdd = filtered_01_bdd;
    dtmc.mgr.deref_node(old_bdd);

    add_dead_end_self_loops(dtmc, reachable);

    dtmc.mgr.deref_node(reachable);
    dtmc.mgr.deref_node(trans_bdd);
}
