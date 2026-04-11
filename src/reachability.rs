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
    dtmc.mgr.deref_node(reachable);
    dtmc.mgr.deref_node(trans_bdd);
}

pub fn count_transitions_minterms(dtmc: &mut SymbolicDTMC) -> u64 {
    let (curr_indices, next_indices) = curr_next_var_indices(dtmc);
    dtmc.mgr.bdd_count_minterms(
        dtmc.transitions_01_bdd,
        (curr_indices.len() + next_indices.len()) as u32,
    )
}
