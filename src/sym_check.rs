//! Symbolic probabilistic model checking for supported DTMC path properties.
//!
//! Currently supported:
//! - `P=? [X phi]`
//! - `P=? [phi1 U<=k phi2]`
//! - `P=? [phi1 U phi2]`
//!
//! This module computes an ADD that maps each current state to its probability,
//! then evaluates that ADD in the (single) initial state.

use anyhow::{bail, Result};
use tracing::{debug, info, trace};

use crate::ast::{Expr, PathFormula, Property};
use crate::constr_symbolic::translate_expr;
use crate::ref_manager::local_roots_guard::LocalRootsGuard;
use crate::ref_manager::{AddNode, BddNode};
use crate::symbolic_dtmc::SymbolicDTMC;

#[derive(Clone, Debug)]
pub enum PropertyEvaluation {
    Probability(f64),
    Unsupported(&'static str),
}

/// Converts a boolean state formula into a current-state BDD.\
/// __Refs__: result\
/// __Derefs__: none
fn state_formula_to_bdd(dtmc: &mut SymbolicDTMC, expr: &Expr) -> BddNode {
    trace!("Translating state formula to BDD: {}", expr);
    let expr_add = translate_expr(expr, dtmc);
    dtmc.mgr.add_to_bdd(expr_add)
}

/// Evaluates an ADD of state values at the initial state by traversing the DD.\
/// __Refs__: none\
/// __Derefs__: values
fn evaluate_add_in_initial_state(dtmc: &mut SymbolicDTMC, values: AddNode) -> f64 {
    let init = dtmc.get_init_bdd();
    let inputs = dtmc
        .mgr
        .extract_leftmost_path_from_bdd(init)
        .expect("initial-state BDD must be satisfiable");

    dtmc.mgr.add_eval_value(values, &inputs)
}

/// Computes the probability ADD for `P=? [X phi]`.
/// __Refs__: result\
/// __Derefs__: none
fn check_next_probability_add(dtmc: &mut SymbolicDTMC, phi: &Expr) -> AddNode {
    let mut guard = LocalRootsGuard::new();
    let phi_bdd = state_formula_to_bdd(dtmc, phi);
    crate::new_protected!(guard, phi_add, dtmc.mgr.bdd_to_add(phi_bdd));
    crate::new_protected!(
        guard,
        curr_to_next_swap_map,
        dtmc.mgr
            .get_swap_map_for_indices(&dtmc.curr_var_indices, &dtmc.next_var_indices)
    );
    crate::new_protected!(
        guard,
        phi_next,
        dtmc.mgr
            .add_compose_with_map(phi_add, curr_to_next_swap_map)
    );
    crate::new_protected!(
        guard,
        next_var_set,
        dtmc.mgr.get_var_set_for_indices(&dtmc.next_var_indices)
    );

    dtmc.mgr
        .add_matrix_multiply_with_var_set(dtmc.transitions.get(), phi_next, next_var_set)
}

/// Computes the probability ADD for `P=? [phi1 U<=k phi2]`.\
/// __Refs__: result\
/// __Derefs__: none
fn check_bounded_until_probability_add(
    dtmc: &mut SymbolicDTMC,
    phi1: &Expr,
    phi2: &Expr,
    k: u32,
) -> AddNode {
    info!("Checking bounded until with bound k={}", k);
    let mut guard = LocalRootsGuard::new();

    let phi1_bdd = state_formula_to_bdd(dtmc, phi1);
    let phi2_bdd = state_formula_to_bdd(dtmc, phi2);

    crate::new_protected!(guard, s_yes_add, dtmc.mgr.bdd_to_add(phi2_bdd));

    let not_phi2 = dtmc.mgr.bdd_not(phi2_bdd);
    let phi1_and_not_phi2 = dtmc.mgr.bdd_and(phi1_bdd, not_phi2);

    let reachable = dtmc.get_reachable_bdd();
    let s_question = dtmc.mgr.bdd_and(reachable, phi1_and_not_phi2);
    let s_question_add = dtmc.mgr.bdd_to_add(s_question);

    crate::new_protected!(
        guard,
        t_question,
        dtmc.mgr.add_times(s_question_add, dtmc.transitions.get())
    );
    crate::new_protected!(
        guard,
        curr_to_next_swap_map,
        dtmc.mgr
            .get_swap_map_for_indices(&dtmc.curr_var_indices, &dtmc.next_var_indices)
    );
    crate::new_protected!(
        guard,
        next_var_set,
        dtmc.mgr.get_var_set_for_indices(&dtmc.next_var_indices)
    );

    crate::new_protected!(guard, res_add, s_yes_add);
    for i in 1..=k {
        let mut iter_guard = LocalRootsGuard::new();
        trace!("Bounded-until iteration {}/{}", i, k);
        crate::new_protected!(
            iter_guard,
            renamed,
            dtmc.mgr
                .add_compose_with_map(res_add, curr_to_next_swap_map)
        );

        crate::new_protected!(
            iter_guard,
            stepped,
            dtmc.mgr
                .add_matrix_multiply_with_var_set(t_question, renamed, next_var_set)
        );

        let s_yes_term = s_yes_add;
        res_add = dtmc.mgr.add_plus(stepped, s_yes_term);
    }
    res_add
}

/// __Refs__: result\
/// __Derefs__: a, b, init
fn solve_jacobi(dtmc: &mut SymbolicDTMC, a: AddNode, b: AddNode, init: AddNode) -> AddNode {
    let mut guard = LocalRootsGuard::new();
    crate::new_protected!(
        guard,
        curr_to_next_swap_map,
        dtmc.mgr
            .get_swap_map_for_indices(&dtmc.curr_var_indices, &dtmc.next_var_indices)
    );
    crate::new_protected!(
        guard,
        next_var_set,
        dtmc.mgr.get_var_set_for_indices(&dtmc.next_var_indices)
    );

    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    let identity_add = dtmc.mgr.bdd_to_add(identity_bdd);

    let a_diag = dtmc.mgr.add_times(a, identity_add);

    let ones = dtmc.mgr.add_const(1.0);
    let d = dtmc
        .mgr
        .add_matrix_multiply_with_var_set(a_diag, ones, next_var_set);

    let d = dtmc.mgr.add_max_abstract(d, dtmc.next_var_cube.get());

    let neg_one = dtmc.mgr.add_const(-1.0);
    let a_neg = dtmc.mgr.add_times(a, neg_one);

    let not_identity_bdd = dtmc.mgr.bdd_not(identity_bdd);
    let not_identity_add = dtmc.mgr.bdd_to_add(not_identity_bdd);
    let a_off_diag = dtmc.mgr.add_times(a_neg, not_identity_add);

    crate::new_protected!(guard, a_prime, dtmc.mgr.add_divide(a_off_diag, d));

    crate::new_protected!(guard, b_prime, dtmc.mgr.add_divide(b, d));

    crate::new_protected!(guard, sol, init);
    let mut iterations = 0usize;
    loop {
        let mut iter_guard = LocalRootsGuard::new();
        iterations += 1;
        crate::new_protected!(
            iter_guard,
            sol_next,
            dtmc.mgr.add_compose_with_map(sol, curr_to_next_swap_map)
        );

        crate::new_protected!(
            iter_guard,
            matmul,
            dtmc.mgr
                .add_matrix_multiply_with_var_set(a_prime, sol_next, next_var_set)
        );

        let sol_prime = dtmc.mgr.add_plus(matmul, b_prime);

        if dtmc
            .mgr
            .add_equal_sup_norm(sol, sol_prime, dtmc.mgr.epsilon())
        {
            info!("Jacobi converged in {} iterations", iterations);
            return sol_prime;
        }

        sol = sol_prime;
    }
}

/// __Refs__: result\
/// __Derefs__: phi1, phi2
fn prob0(dtmc: &mut SymbolicDTMC, phi1: BddNode, phi2: BddNode) -> BddNode {
    let mut guard = LocalRootsGuard::new();
    crate::new_protected!(guard, phi1_rooted, phi1);
    crate::new_protected!(
        guard,
        curr_to_next_swap_map,
        dtmc.mgr
            .get_swap_map_for_indices(&dtmc.curr_var_indices, &dtmc.next_var_indices)
    );

    crate::new_protected!(guard, sol, phi2);
    let mut iterations = 0usize;

    loop {
        let mut iter_guard = LocalRootsGuard::new();
        iterations += 1;

        crate::new_protected!(
            iter_guard,
            sol_next,
            dtmc.mgr.bdd_compose_with_map(sol, curr_to_next_swap_map)
        );

        let t_01 = dtmc.get_transitions_01();
        crate::new_protected!(
            iter_guard,
            post,
            dtmc.mgr
                .bdd_and_then_existsabs(t_01, sol_next, dtmc.next_var_cube.get())
        );

        crate::new_protected!(iter_guard, step, dtmc.mgr.bdd_and(phi1_rooted, post));

        let sol_prime = dtmc.mgr.bdd_or(sol, step);

        if sol_prime == sol {
            sol = sol_prime;
            break;
        }
        sol = sol_prime;
    }

    trace!("prob0 converged in {} iterations", iterations);

    let reachable = dtmc.get_reachable_bdd();
    let not_sol = dtmc.mgr.bdd_not(sol);
    dtmc.mgr.bdd_and(reachable, not_sol)
}

/// __Refs__: result\
/// __Derefs__: phi1, phi2, s_no
fn prob1(dtmc: &mut SymbolicDTMC, phi1: BddNode, phi2: BddNode, s_no: BddNode) -> BddNode {
    let mut guard = LocalRootsGuard::new();
    crate::new_protected!(
        guard,
        curr_to_next_swap_map,
        dtmc.mgr
            .get_swap_map_for_indices(&dtmc.curr_var_indices, &dtmc.next_var_indices)
    );

    let not_phi2 = dtmc.mgr.bdd_not(phi2);
    crate::new_protected!(guard, phi1_and_not_phi2, dtmc.mgr.bdd_and(phi1, not_phi2));

    crate::new_protected!(guard, sol, s_no);
    let mut iterations = 0usize;

    loop {
        let mut iter_guard = LocalRootsGuard::new();
        iterations += 1;

        crate::new_protected!(
            iter_guard,
            sol_next,
            dtmc.mgr.bdd_compose_with_map(sol, curr_to_next_swap_map)
        );

        let t_01 = dtmc.get_transitions_01();
        crate::new_protected!(
            iter_guard,
            post,
            dtmc.mgr
                .bdd_and_then_existsabs(t_01, sol_next, dtmc.next_var_cube.get())
        );

        crate::new_protected!(iter_guard, step, dtmc.mgr.bdd_and(phi1_and_not_phi2, post));

        let sol_prime = dtmc.mgr.bdd_or(sol, step);

        if sol_prime == sol {
            sol = sol_prime;
            break;
        }
        sol = sol_prime;
    }

    trace!("prob1 converged in {} iterations", iterations);

    let reachable = dtmc.get_reachable_bdd();
    let not_sol = dtmc.mgr.bdd_not(sol);
    dtmc.mgr.bdd_and(reachable, not_sol)
}

/// __Refs__: result\
/// __Derefs__: none
fn check_unbounded_until_probability_add(
    dtmc: &mut SymbolicDTMC,
    phi1: &Expr,
    phi2: &Expr,
) -> AddNode {
    info!("Checking unbounded until");
    let mut guard = LocalRootsGuard::new();

    let phi1_bdd = state_formula_to_bdd(dtmc, phi1);
    let phi2_bdd = state_formula_to_bdd(dtmc, phi2);

    crate::new_protected!(guard, s_no, prob0(dtmc, phi1_bdd, phi2_bdd));
    crate::new_protected!(guard, s_yes, prob1(dtmc, phi1_bdd, phi2_bdd, s_no));

    let no_or_yes = dtmc.mgr.bdd_or(s_no, s_yes); // consume s_no
    let not_no_or_yes = dtmc.mgr.bdd_not(no_or_yes);

    let reachable = dtmc.get_reachable_bdd();
    let s_question = dtmc.mgr.bdd_and(reachable, not_no_or_yes);

    let s_question_add = dtmc.mgr.bdd_to_add(s_question);

    let t_question = dtmc.mgr.add_times(dtmc.transitions.get(), s_question_add);

    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    let identity_add = dtmc.mgr.bdd_to_add(identity_bdd);

    let a = dtmc.mgr.add_minus(identity_add, t_question);

    let b = dtmc.mgr.bdd_to_add(s_yes); // consume s_yes

    solve_jacobi(dtmc, a, b, b)
}

/// Evaluates one property at the single initial state.
pub fn evaluate_property_at_initial_state(
    dtmc: &mut SymbolicDTMC,
    property: &Property,
) -> Result<PropertyEvaluation> {
    match property {
        Property::ProbQuery(PathFormula::Next(phi)) => {
            info!("Checking probability next property: {}", property);
            let probability_add = check_next_probability_add(dtmc, phi);
            let value = evaluate_add_in_initial_state(dtmc, probability_add);
            debug!("Computed P=? [X phi] value at initial state: {}", value);
            Ok(PropertyEvaluation::Probability(value))
        }
        Property::ProbQuery(PathFormula::Until {
            lhs,
            rhs,
            bound: Some(k_expr),
        }) => {
            let k = match k_expr.as_ref() {
                Expr::IntLit(v) if *v >= 0 => *v as u32,
                _ => bail!("Bounded-until bound must be a non-negative integer literal"),
            };
            info!("Checking bounded-until property: {}", property);
            let probability_add = check_bounded_until_probability_add(dtmc, lhs, rhs, k);
            let value = evaluate_add_in_initial_state(dtmc, probability_add);
            debug!(
                "Computed P=? [phi1 U<=k phi2] value at initial state: {}",
                value
            );
            Ok(PropertyEvaluation::Probability(value))
        }
        Property::ProbQuery(PathFormula::Until {
            lhs,
            rhs,
            bound: None,
        }) => {
            info!("Checking unbounded-until property: {}", property);
            let probability_add = check_unbounded_until_probability_add(dtmc, lhs, rhs);
            let value = evaluate_add_in_initial_state(dtmc, probability_add);
            debug!(
                "Computed P=? [phi1 U phi2] value at initial state: {}",
                value
            );
            Ok(PropertyEvaluation::Probability(value))
        }
        Property::RewardQuery(_) => Ok(PropertyEvaluation::Unsupported(
            "Reward properties are not supported yet",
        )),
    }
}
