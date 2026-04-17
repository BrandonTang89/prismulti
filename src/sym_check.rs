//! Symbolic probabilistic model checking for supported DTMC path properties.
//!
//! Currently supported:
//! - `P=? [X phi]`
//! - `P=? [phi1 U<=k phi2]`
//! - `P=? [phi1 U phi2]`
//!
//! This module computes an ADD that maps each current state to its probability,
//! then evaluates that ADD in the (single) initial state.

use anyhow::{Result, bail};
use tracing::{debug, info, trace};

use crate::ast::{Expr, PathFormula, Property};
use crate::constr_symbolic::translate_expr;
use crate::dd_manager::dd;
use crate::dd_manager::{AddNode, BddNode};
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
    crate::protected_add!(expr_add, translate_expr(expr, dtmc));
    dd::add_to_bdd(expr_add.get())
}

/// Evaluates an ADD of state values at the initial state by traversing the DD.\
/// __Refs__: none\
/// __Derefs__: values
fn evaluate_add_in_initial_state(dtmc: &mut SymbolicDTMC, values: AddNode) -> f64 {
    let init = dtmc.get_init_bdd();
    let inputs = dd::extract_leftmost_path_from_bdd(&dtmc.mgr, init)
        .expect("initial-state BDD must be satisfiable");

    dd::add_eval_value(&dtmc.mgr, values, &inputs)
}

/// Computes the probability ADD for `P=? [X phi]`.
/// __Refs__: result\
/// __Derefs__: none
fn check_next_probability_add(dtmc: &mut SymbolicDTMC, phi: &Expr) -> AddNode {
    crate::protected_bdd!(phi_bdd, state_formula_to_bdd(dtmc, phi));
    crate::protected_add!(phi_add, dd::bdd_to_add(phi_bdd.get()));
    crate::protected_map!(
        curr_to_next_swap_map,
        dd::get_swap_map_for_indices(
            &mut dtmc.mgr,
            &dtmc.curr_var_indices,
            &dtmc.next_var_indices,
        )
    );
    crate::protected_add!(
        phi_next,
        dd::add_compose_with_map(phi_add.get(), curr_to_next_swap_map.get())
    );
    crate::protected_var_set!(
        next_var_set,
        dd::get_var_set_for_indices(&dtmc.next_var_indices)
    );

    dd::add_matrix_multiply_with_var_set(dtmc.transitions.get(), phi_next.get(), next_var_set.get())
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
    crate::protected_bdd!(phi1_bdd, state_formula_to_bdd(dtmc, phi1));
    crate::protected_bdd!(phi2_bdd, state_formula_to_bdd(dtmc, phi2));

    crate::protected_add!(s_yes_add, dd::bdd_to_add(phi2_bdd.get()));

    crate::protected_bdd!(not_phi2, dd::bdd_not(phi2_bdd.get()));
    crate::protected_bdd!(
        phi1_and_not_phi2,
        dd::bdd_and(phi1_bdd.get(), not_phi2.get())
    );

    let reachable = dtmc.get_reachable_bdd();
    crate::protected_bdd!(s_question, dd::bdd_and(reachable, phi1_and_not_phi2.get()));
    crate::protected_add!(s_question_add, dd::bdd_to_add(s_question.get()));

    crate::protected_add!(
        t_question,
        dd::add_times(s_question_add.get(), dtmc.transitions.get())
    );
    crate::protected_map!(
        curr_to_next_swap_map,
        dd::get_swap_map_for_indices(
            &mut dtmc.mgr,
            &dtmc.curr_var_indices,
            &dtmc.next_var_indices,
        )
    );
    crate::protected_var_set!(
        next_var_set,
        dd::get_var_set_for_indices(&dtmc.next_var_indices)
    );

    crate::protected_add!(res_add, s_yes_add.get());
    for i in 1..=k {
        trace!("Bounded-until iteration {}/{}", i, k);
        crate::protected_add!(
            renamed,
            dd::add_compose_with_map(res_add.get(), curr_to_next_swap_map.get())
        );

        crate::protected_add!(
            stepped,
            dd::add_matrix_multiply_with_var_set(
                t_question.get(),
                renamed.get(),
                next_var_set.get()
            )
        );

        res_add.set(dd::add_plus(stepped.get(), s_yes_add.get()));
    }
    res_add.get()
}

/// __Refs__: result\
/// __Derefs__: a, b, init
fn solve_jacobi(dtmc: &mut SymbolicDTMC, a: AddNode, b: AddNode, init: AddNode) -> AddNode {
    crate::protected_map!(
        curr_to_next_swap_map,
        dd::get_swap_map_for_indices(
            &mut dtmc.mgr,
            &dtmc.curr_var_indices,
            &dtmc.next_var_indices,
        )
    );
    crate::protected_var_set!(
        next_var_set,
        dd::get_var_set_for_indices(&dtmc.next_var_indices)
    );

    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    crate::protected_add!(identity_add, dd::bdd_to_add(identity_bdd));

    crate::protected_add!(a_diag, dd::add_times(a, identity_add.get()));

    crate::protected_add!(ones, dd::add_const(1.0));
    crate::protected_add!(
        d,
        dd::add_matrix_multiply_with_var_set(a_diag.get(), ones.get(), next_var_set.get())
    );

    crate::protected_add!(d, dd::add_max_abstract(d.get(), dtmc.next_var_cube.get()));

    crate::protected_add!(neg_one, dd::add_const(-1.0));
    crate::protected_add!(a_neg, dd::add_times(a, neg_one.get()));

    crate::protected_bdd!(not_identity_bdd, dd::bdd_not(identity_bdd));
    crate::protected_add!(not_identity_add, dd::bdd_to_add(not_identity_bdd.get()));
    crate::protected_add!(
        a_off_diag,
        dd::add_times(a_neg.get(), not_identity_add.get())
    );

    crate::protected_add!(a_prime, dd::add_divide(a_off_diag.get(), d.get()));

    crate::protected_add!(b_prime, dd::add_divide(b, d.get()));

    crate::protected_add!(sol, init);
    let mut iterations = 0usize;
    loop {
        iterations += 1;
        crate::protected_add!(
            sol_next,
            dd::add_compose_with_map(sol.get(), curr_to_next_swap_map.get())
        );

        crate::protected_add!(
            matmul,
            dd::add_matrix_multiply_with_var_set(a_prime.get(), sol_next.get(), next_var_set.get())
        );

        crate::protected_add!(sol_prime, dd::add_plus(matmul.get(), b_prime.get()));

        if dd::add_equal_sup_norm(sol.get(), sol_prime.get(), dd::epsilon()) {
            info!("Jacobi converged in {} iterations", iterations);
            return sol_prime.get();
        }

        sol.set(sol_prime.get());
    }
}

/// __Refs__: result\
/// __Derefs__: phi1, phi2
fn prob0(dtmc: &mut SymbolicDTMC, phi1: BddNode, phi2: BddNode) -> BddNode {
    crate::protected_map!(
        curr_to_next_swap_map,
        dd::get_swap_map_for_indices(
            &mut dtmc.mgr,
            &dtmc.curr_var_indices,
            &dtmc.next_var_indices,
        )
    );

    crate::protected_bdd!(sol, phi2);
    let mut iterations = 0usize;

    loop {
        iterations += 1;

        crate::protected_bdd!(
            sol_next,
            dd::bdd_compose_with_map(sol.get(), curr_to_next_swap_map.get())
        );

        let t_01 = dtmc.get_transitions_01();
        crate::protected_bdd!(
            post,
            dd::bdd_and_then_existsabs(t_01, sol_next.get(), dtmc.next_var_cube.get())
        );

        crate::protected_bdd!(step, dd::bdd_and(phi1, post.get()));

        crate::protected_bdd!(sol_prime, dd::bdd_or(sol.get(), step.get()));

        if sol_prime.get() == sol.get() {
            sol.set(sol_prime.get());
            break;
        }
        sol.set(sol_prime.get());
    }

    trace!("prob0 converged in {} iterations", iterations);

    let reachable = dtmc.get_reachable_bdd();
    crate::protected_bdd!(not_sol, dd::bdd_not(sol.get()));
    dd::bdd_and(reachable, not_sol.get())
}

/// __Refs__: result\
/// __Derefs__: phi1, phi2, s_no
fn prob1(dtmc: &mut SymbolicDTMC, phi1: BddNode, phi2: BddNode, s_no: BddNode) -> BddNode {
    crate::protected_map!(
        curr_to_next_swap_map,
        dd::get_swap_map_for_indices(
            &mut dtmc.mgr,
            &dtmc.curr_var_indices,
            &dtmc.next_var_indices,
        )
    );

    crate::protected_bdd!(not_phi2, dd::bdd_not(phi2));
    crate::protected_bdd!(phi1_and_not_phi2, dd::bdd_and(phi1, not_phi2.get()));

    crate::protected_bdd!(sol, s_no);
    let mut iterations = 0usize;

    loop {
        iterations += 1;

        crate::protected_bdd!(
            sol_next,
            dd::bdd_compose_with_map(sol.get(), curr_to_next_swap_map.get())
        );

        let t_01 = dtmc.get_transitions_01();
        crate::protected_bdd!(
            post,
            dd::bdd_and_then_existsabs(t_01, sol_next.get(), dtmc.next_var_cube.get())
        );

        crate::protected_bdd!(step, dd::bdd_and(phi1_and_not_phi2.get(), post.get()));

        crate::protected_bdd!(sol_prime, dd::bdd_or(sol.get(), step.get()));

        if sol_prime.get() == sol.get() {
            sol.set(sol_prime.get());
            break;
        }
        sol.set(sol_prime.get());
    }

    trace!("prob1 converged in {} iterations", iterations);

    let reachable = dtmc.get_reachable_bdd();
    crate::protected_bdd!(not_sol, dd::bdd_not(sol.get()));
    dd::bdd_and(reachable, not_sol.get())
}

/// __Refs__: result\
/// __Derefs__: none
fn check_unbounded_until_probability_add(
    dtmc: &mut SymbolicDTMC,
    phi1: &Expr,
    phi2: &Expr,
) -> AddNode {
    info!("Checking unbounded until");
    crate::protected_bdd!(phi1_bdd, state_formula_to_bdd(dtmc, phi1));
    crate::protected_bdd!(phi2_bdd, state_formula_to_bdd(dtmc, phi2));

    crate::protected_bdd!(s_no, prob0(dtmc, phi1_bdd.get(), phi2_bdd.get()));
    crate::protected_bdd!(
        s_yes,
        prob1(dtmc, phi1_bdd.get(), phi2_bdd.get(), s_no.get())
    );

    crate::protected_bdd!(no_or_yes, dd::bdd_or(s_no.get(), s_yes.get()));
    crate::protected_bdd!(not_no_or_yes, dd::bdd_not(no_or_yes.get()));

    let reachable = dtmc.get_reachable_bdd();
    crate::protected_bdd!(s_question, dd::bdd_and(reachable, not_no_or_yes.get()));

    crate::protected_add!(s_question_add, dd::bdd_to_add(s_question.get()));

    crate::protected_add!(
        t_question,
        dd::add_times(dtmc.transitions.get(), s_question_add.get())
    );

    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    crate::protected_add!(identity_add, dd::bdd_to_add(identity_bdd));

    crate::protected_add!(a, dd::add_minus(identity_add.get(), t_question.get()));

    crate::protected_add!(b, dd::bdd_to_add(s_yes.get()));

    solve_jacobi(dtmc, a.get(), b.get(), b.get())
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
