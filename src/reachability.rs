use crate::dd;
use crate::ref_manager::protected_local::{ProtectedBddLocal, ProtectedMapLocal};
use crate::symbolic_dtmc::SymbolicDTMC;
use tracing::info;

pub fn compute_reachable_and_filter(dtmc: &mut SymbolicDTMC) {
    let init = dtmc.get_init_bdd();

    let mut reachable = ProtectedBddLocal::new(init);

    let trans_rel = ProtectedBddLocal::new(dd::add_to_bdd(&mut dtmc.mgr, dtmc.transitions.get()));
    let next_to_curr_swap_map = dd::get_swap_map_for_indices(
        &mut dtmc.mgr,
        &dtmc.next_var_indices,
        &dtmc.curr_var_indices,
    );
    let next_to_curr_swap_map_rooted = ProtectedMapLocal::new(next_to_curr_swap_map);

    let mut iterations = 0usize;

    loop {
        iterations += 1;
        let old = ProtectedBddLocal::new(reachable.get());

        let image_next = dd::bdd_and_then_existsabs(
            &dtmc.mgr,
            old.get(),
            trans_rel.get(),
            dtmc.curr_var_set.get(),
        );
        let image_curr = dd::bdd_compose_with_map(
            &mut dtmc.mgr,
            image_next,
            next_to_curr_swap_map_rooted.get(),
        );
        let new_reachable = dd::bdd_or(&dtmc.mgr, old.get(), image_curr);

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
