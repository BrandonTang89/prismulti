use crate::ast::{Expr, VarDecl, VarType};
use crate::ref_manager::BddNode;
use crate::symbolic_dtmc::SymbolicDTMC;

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

fn build_init_add01(dtmc: &mut SymbolicDTMC) -> BddNode {
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
                    BddNode(bit)
                } else {
                    dtmc.mgr.bdd_not(BddNode(bit))
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
                curr_indices.push(dtmc.mgr.read_var_index(curr));
                next_indices.push(dtmc.mgr.read_var_index(next));
            }
        }
    }

    (curr_indices, next_indices)
}

fn build_curr_next_identity_add01(dtmc: &mut SymbolicDTMC) -> BddNode {
    let mut ident = dtmc.mgr.bdd_one();
    for module in &dtmc.ast.modules {
        for var_decl in &module.local_vars {
            let var_name = &var_decl.name;
            let curr_nodes = dtmc.var_curr_nodes[var_name].clone();
            let next_nodes = dtmc.var_next_nodes[var_name].clone();
            for (curr, next) in curr_nodes.into_iter().zip(next_nodes.into_iter()) {
                dtmc.mgr.ref_node(curr);
                dtmc.mgr.ref_node(next);
                let eq = dtmc.mgr.bdd_equals(BddNode(curr), BddNode(next));
                ident = dtmc.mgr.bdd_and(ident, eq);
            }
        }
    }
    ident
}

fn add_dead_end_self_loops(dtmc: &mut SymbolicDTMC, reachable: BddNode) {
    dtmc.mgr.ref_node(dtmc.transitions_01_add.0);
    let out_curr = dtmc
        .mgr
        .bdd_or_abstract(dtmc.transitions_01_add, dtmc.next_var_cube);

    dtmc.mgr.ref_node(out_curr.0);
    let not_out_curr = dtmc.mgr.bdd_not(out_curr);

    dtmc.mgr.ref_node(reachable.0);
    let dead_end_curr = dtmc.mgr.bdd_and(reachable, not_out_curr);
    dtmc.mgr.deref_node(out_curr.0);

    dtmc.mgr.ref_node(dead_end_curr.0);
    let dead_end_add_for_count = dtmc.mgr.bdd_to_add(dead_end_curr);
    dtmc.mgr.ref_node(dtmc.curr_var_cube.0);
    let curr_cube_add = dtmc.mgr.bdd_to_add(dtmc.curr_var_cube);
    let dead_end_count_add = dtmc
        .mgr
        .add_sum_abstract(dead_end_add_for_count, curr_cube_add);
    dtmc.mgr.deref_node(curr_cube_add.0);
    let dead_end_count = dtmc
        .mgr
        .add_value(dead_end_count_add.0)
        .unwrap_or(0.0)
        .round() as u64;
    dtmc.mgr.deref_node(dead_end_count_add.0);

    if dead_end_count > 0 {
        let curr_next_eq = build_curr_next_identity_add01(dtmc);
        dtmc.mgr.ref_node(dead_end_curr.0);
        let self_loops = dtmc.mgr.bdd_and(dead_end_curr, curr_next_eq);

        let old_rel = dtmc.transitions_01_add;
        dtmc.mgr.ref_node(old_rel.0);
        dtmc.mgr.ref_node(self_loops.0);
        dtmc.transitions_01_add = dtmc.mgr.bdd_or(old_rel, self_loops);
        dtmc.mgr.deref_node(old_rel.0);

        dtmc.mgr.ref_node(self_loops.0);
        let self_loops_add = dtmc.mgr.bdd_to_add(self_loops);
        let old_add = dtmc.transitions;
        dtmc.mgr.ref_node(old_add.0);
        dtmc.transitions = dtmc.mgr.add_plus(old_add, self_loops_add);
        dtmc.mgr.deref_node(old_add.0);
        dtmc.mgr.deref_node(self_loops.0);
    }

    println!("Added self-loops to {} dead-end states", dead_end_count);
    dtmc.mgr.deref_node(dead_end_curr.0);
}

pub fn compute_reachable_and_filter(dtmc: &mut SymbolicDTMC) {
    let mut reachable = build_init_add01(dtmc);

    dtmc.mgr.ref_node(dtmc.transitions.0);
    let trans_rel = dtmc.mgr.add_to_bdd(dtmc.transitions);

    let (curr_indices, next_indices) = curr_next_var_indices(dtmc);
    let mut iterations = 0usize;

    loop {
        iterations += 1;
        let old = reachable;

        dtmc.mgr.ref_node(old.0);
        dtmc.mgr.ref_node(trans_rel.0);
        let image_next = dtmc
            .mgr
            .bdd_and_abstract(old, trans_rel, dtmc.curr_var_cube);
        let image_curr = dtmc
            .mgr
            .bdd_swap_variables(image_next, &next_indices, &curr_indices);
        let new_reachable = dtmc.mgr.bdd_or(old, image_curr);

        reachable = new_reachable;
        if new_reachable == old {
            break;
        }
    }

    dtmc.mgr.ref_node(reachable.0);
    let reachable_add_for_count = dtmc.mgr.bdd_to_add(reachable);
    dtmc.mgr.ref_node(dtmc.curr_var_cube.0);
    let curr_cube_add = dtmc.mgr.bdd_to_add(dtmc.curr_var_cube);
    let reachable_count_add = dtmc
        .mgr
        .add_sum_abstract(reachable_add_for_count, curr_cube_add);
    dtmc.mgr.deref_node(curr_cube_add.0);
    let reachable_states = dtmc
        .mgr
        .add_value(reachable_count_add.0)
        .unwrap_or(0.0)
        .round() as u64;
    dtmc.reachable_states = reachable_states;
    dtmc.mgr.deref_node(dtmc.reachable.0);
    dtmc.mgr.ref_node(reachable.0);
    dtmc.reachable = reachable;
    dtmc.mgr.deref_node(reachable_count_add.0);

    println!(
        "Reachability (BFS): {} iterations, reachable states: {}",
        iterations, reachable_states
    );

    dtmc.mgr.ref_node(reachable.0);
    let reachable_add = dtmc.mgr.bdd_to_add(reachable);
    let old_transitions = dtmc.transitions;
    dtmc.mgr.ref_node(old_transitions.0);
    dtmc.transitions = dtmc.mgr.add_times(old_transitions, reachable_add);
    dtmc.mgr.deref_node(old_transitions.0);

    dtmc.mgr.ref_node(dtmc.transitions.0);
    let filtered_01 = dtmc.mgr.add_to_bdd(dtmc.transitions);
    let old = dtmc.transitions_01_add;
    dtmc.transitions_01_add = filtered_01;
    dtmc.mgr.deref_node(old.0);

    add_dead_end_self_loops(dtmc, reachable);

    dtmc.mgr.deref_node(reachable.0);
    dtmc.mgr.deref_node(trans_rel.0);
}
