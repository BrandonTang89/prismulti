use crate::dd_manager::dd;
use crate::symbolic_dtmc::SymbolicDTMC;
use tracing::info;

pub fn compute_reachable_and_filter(dtmc: &mut SymbolicDTMC) {
    let init = dtmc.get_init_bdd();

    crate::protected_bdd!(reachable, init);

    crate::protected_bdd!(trans_rel, dd::add_to_bdd(dtmc.transitions.get()));
    let next_to_curr_swap_map = dd::get_swap_map_for_indices(
        &mut dtmc.mgr,
        &dtmc.next_var_indices,
        &dtmc.curr_var_indices,
    );
    crate::protected_map!(next_to_curr_swap_map_rooted, next_to_curr_swap_map);

    let mut iterations = 0usize;

    loop {
        iterations += 1;
        crate::protected_bdd!(old, reachable.get());

        let image_next =
            dd::bdd_and_then_existsabs(old.get(), trans_rel.get(), dtmc.curr_var_set.get());
        let image_curr = dd::bdd_compose_with_map(image_next, next_to_curr_swap_map_rooted.get());
        let new_reachable = dd::bdd_or(old.get(), image_curr);

        reachable.set(new_reachable);
        if new_reachable == old.get() {
            break;
        }
    }

    dtmc.set_reachable_and_filter(reachable.get());

    let reachable_states = dtmc.reachable_state_count();
    info!(
        "Reachability (BFS): {} iterations, reachable states: {}",
        iterations, reachable_states
    );
}
