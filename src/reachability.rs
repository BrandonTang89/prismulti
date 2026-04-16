use crate::symbolic_dtmc::SymbolicDTMC;
use crate::{new_protected, ref_manager::local_roots_guard::LocalRootsGuard};
use tracing::info;

pub fn compute_reachable_and_filter(dtmc: &mut SymbolicDTMC) {
    let mut guard = LocalRootsGuard::new();
    let init = dtmc.get_init_bdd();

    new_protected!(guard, reachable, init);

    new_protected!(
        guard,
        trans_rel,
        dtmc.mgr.add_to_bdd(dtmc.transitions.get())
    );
    let next_to_curr_swap_map = dtmc
        .mgr
        .get_swap_map_for_indices(&dtmc.next_var_indices, &dtmc.curr_var_indices);
    new_protected!(guard, next_to_curr_swap_map_rooted, next_to_curr_swap_map);

    let mut iterations = 0usize;

    loop {
        iterations += 1;
        let old = reachable;

        let image_next = dtmc
            .mgr
            .bdd_and_then_existsabs(old, trans_rel, dtmc.curr_var_set.get());
        let image_curr = dtmc
            .mgr
            .bdd_compose_with_map(image_next, next_to_curr_swap_map_rooted);
        let new_reachable = dtmc.mgr.bdd_or(old, image_curr);

        reachable = new_reachable;
        if new_reachable == old {
            break;
        }
    }

    dtmc.set_reachable_and_filter(reachable);

    let reachable_states = dtmc.reachable_state_count();
    info!(
        "Reachability (BFS): {} iterations, reachable states: {}",
        iterations, reachable_states
    );
}
