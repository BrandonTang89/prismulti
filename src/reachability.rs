use crate::symbolic_dtmc::SymbolicDTMC;

pub fn compute_reachable_and_filter(dtmc: &mut SymbolicDTMC) {
    let init = dtmc.get_init_bdd();

    dtmc.mgr.ref_node(init.0);
    let mut reachable = init;

    dtmc.mgr.ref_node(dtmc.transitions.0);
    let trans_rel = dtmc.mgr.add_to_bdd(dtmc.transitions);

    let mut iterations = 0usize;

    loop {
        iterations += 1;
        let old = reachable;

        dtmc.mgr.ref_node(old.0);
        dtmc.mgr.ref_node(trans_rel.0);
        let image_next = dtmc
            .mgr
            .bdd_and_then_existsabs(old, trans_rel, dtmc.curr_var_cube);
        let image_curr =
            dtmc.mgr
                .bdd_swap_variables(image_next, &dtmc.next_var_indices, &dtmc.curr_var_indices);
        let new_reachable = dtmc.mgr.bdd_or(old, image_curr);

        reachable = new_reachable;
        if new_reachable == old {
            break;
        }
    }

    dtmc.mgr.ref_node(reachable.0);
    dtmc.set_reachable_and_filter(reachable);

    let reachable_states = dtmc.reachable_state_count();
    println!(
        "Reachability (BFS): {} iterations, reachable states: {}",
        iterations, reachable_states
    );

    dtmc.mgr.deref_node(reachable.0);
    dtmc.mgr.deref_node(init.0);
    dtmc.mgr.deref_node(trans_rel.0);
}
