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

use crate::ast::{DTMCProperty, Expr, PathFormula};
use crate::constr_symbolic::translate_expr;
use crate::dd_manager::dd;
use crate::dd_manager::{AddNode, BddNode};
use crate::protected_add;
use crate::protected_bdd;
use crate::symbolic_dtmc::SymbolicDTMC;

#[derive(Clone, Debug)]
pub enum PropertyEvaluation {
    Probability(f64),
    Unsupported(&'static str),
}

/// Converts a boolean state formula into a current-state BDD.
fn state_formula_to_bdd(dtmc: &mut SymbolicDTMC, expr: &Expr) -> BddNode {
    trace!("Translating state formula to BDD: {}", expr);
    protected_add!(expr_add, translate_expr(expr, dtmc));
    protected_bdd!(expr_bdd, dd::add_to_bdd(expr_add.get()));
    dd::bdd_and(expr_bdd.get(), dtmc.get_reachable_bdd())
}

/// Evaluates an ADD of state values at the initial state by traversing the DD.\
fn evaluate_add_in_initial_state(dtmc: &mut SymbolicDTMC, values: AddNode) -> f64 {
    let init = dtmc.get_init_bdd();
    let inputs = dd::extract_leftmost_path_from_bdd(&dtmc.mgr, init)
        .expect("initial-state BDD must be satisfiable");

    dd::add_eval_value(&dtmc.mgr, values, &inputs)
}

/// Computes the probability ADD for `P=? [X phi]`.
fn check_next_probability_add(dtmc: &mut SymbolicDTMC, phi: &Expr) -> AddNode {
    protected_bdd!(phi_bdd, state_formula_to_bdd(dtmc, phi));
    protected_add!(phi_add, dd::bdd_to_add(phi_bdd.get()));

    protected_add!(
        phi_next,
        dd::add_compose_with_map(phi_add.get(), dtmc.curr_to_next_map.get())
    );

    dd::add_matrix_multiply(
        dtmc.transitions.get(),
        phi_next.get(),
        dtmc.next_var_set.get(),
    )
}

/// Computes the probability ADD for `P=? [phi1 U<=k phi2]`.\
fn check_bounded_until_probability_add(
    dtmc: &mut SymbolicDTMC,
    phi1: &Expr,
    phi2: &Expr,
    k: u32,
) -> AddNode {
    info!("Checking bounded until with bound k={}", k);

    protected_add!(
        not_reachable_add,
        dd::bdd_to_add(dd::bdd_not(dtmc.get_reachable_bdd()))
    );

    protected_bdd!(phi1_bdd, state_formula_to_bdd(dtmc, phi1));
    protected_bdd!(phi2_bdd, state_formula_to_bdd(dtmc, phi2));

    protected_add!(
        tmp,
        dd::add_times(dd::bdd_to_add(phi2_bdd.get()), not_reachable_add.get())
    );
    println!("{:?}", dd::terminal_values(tmp.get()));

    protected_add!(s_yes_add, dd::bdd_to_add(phi2_bdd.get()));

    protected_bdd!(not_phi2, dd::bdd_not(phi2_bdd.get()));
    protected_bdd!(
        phi1_and_not_phi2,
        dd::bdd_and(phi1_bdd.get(), not_phi2.get())
    );

    let reachable = dtmc.get_reachable_bdd();
    protected_bdd!(s_question, dd::bdd_and(reachable, phi1_and_not_phi2.get()));
    protected_add!(s_question_add, dd::bdd_to_add(s_question.get()));

    protected_add!(
        t_question,
        dd::add_times(s_question_add.get(), dtmc.transitions.get())
    );

    protected_add!(res_add, s_yes_add.get());
    protected_add!(renamed);
    protected_add!(stepped);

    // Restrict to minimise t_question size
    // Next Variable Restrict - Don't need to fix up
    protected_bdd!(
        reachable_next,
        dd::bdd_swap_variables(dtmc.get_reachable_bdd(), dtmc.curr_to_next_map.get())
    );
    t_question.replace(dd::add_restrict(t_question.get(), reachable_next.get()));

    // Curr Variable Restrict
    t_question.replace(dd::add_restrict(t_question.get(), dtmc.get_reachable_bdd()));

    for i in 1..=k {
        trace!("Bounded-until iteration {}/{}", i, k);
        renamed.replace(dd::add_compose_with_map(
            res_add.get(),
            dtmc.curr_to_next_map.get(),
        ));

        stepped.replace(dd::add_matrix_multiply(
            t_question.get(),
            renamed.get(),
            dtmc.next_var_set.get(),
        ));
        // fix up current variable mask
        stepped.replace(dd::add_mask(stepped.get(), dtmc.get_reachable_bdd()));

        res_add.set(dd::add_plus(stepped.get(), s_yes_add.get()));
    }
    res_add.get()
}

fn solve_jacobi(dtmc: &mut SymbolicDTMC, a: AddNode, b: AddNode, init: AddNode) -> AddNode {
    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    protected_add!(identity_add, dd::bdd_to_add(identity_bdd));

    protected_add!(a_diag, dd::add_times(a, identity_add.get()));

    protected_add!(ones, dd::add_const(1.0));
    protected_add!(
        d,
        dd::add_matrix_multiply(a_diag.get(), ones.get(), dtmc.next_var_set.get())
    );

    protected_add!(d, dd::add_max_abstract(d.get(), dtmc.next_var_set.get()));

    protected_add!(neg_one, dd::add_const(-1.0));
    protected_add!(a_neg, dd::add_times(a, neg_one.get()));

    protected_bdd!(not_identity_bdd, dd::bdd_not(identity_bdd));
    protected_add!(not_identity_add, dd::bdd_to_add(not_identity_bdd.get()));
    protected_add!(
        a_off_diag,
        dd::add_times(a_neg.get(), not_identity_add.get())
    );

    protected_add!(a_prime, dd::add_divide(a_off_diag.get(), d.get()));
    protected_add!(b_prime, dd::add_divide(b, d.get()));
    protected_add!(sol, init);

    protected_add!(sol_next);
    protected_add!(matmul);
    protected_add!(sol_prime);

    // Restrict to minimise a_prime size
    // Next Variable Restrict - Don't need to fix up
    protected_bdd!(
        reachable_next,
        dd::bdd_swap_variables(dtmc.get_reachable_bdd(), dtmc.curr_to_next_map.get())
    );
    a_prime.replace(dd::add_restrict(a_prime.get(), reachable_next.get()));

    // Curr Variable Restrict
    a_prime.replace(dd::add_restrict(a_prime.get(), dtmc.get_reachable_bdd()));

    let mut iterations = 0usize;
    loop {
        iterations += 1;
        sol_next.replace(dd::add_compose_with_map(
            sol.get(),
            dtmc.curr_to_next_map.get(),
        ));

        matmul.replace(dd::add_matrix_multiply(
            a_prime.get(),
            sol_next.get(),
            dtmc.next_var_set.get(),
        ));

        // Fix up current variable restrict
        matmul.replace(dd::add_mask(matmul.get(), dtmc.get_reachable_bdd()));

        sol_prime.replace(dd::add_plus(matmul.get(), b_prime.get()));

        if dd::add_equal_sup_norm(sol.get(), sol_prime.get(), dd::epsilon()) {
            info!("Jacobi converged in {} iterations", iterations);
            return sol_prime.get();
        }

        sol.set(sol_prime.get());
    }
}

fn prob0(dtmc: &mut SymbolicDTMC, phi1: BddNode, phi2: BddNode) -> BddNode {
    protected_bdd!(sol, phi2);
    protected_bdd!(sol_next, sol.get());
    protected_bdd!(post, sol.get());
    protected_bdd!(step, sol.get());
    protected_bdd!(sol_prime, sol.get());
    let mut iterations = 0usize;

    loop {
        iterations += 1;

        sol_next.replace(dd::bdd_compose_with_map(
            sol.get(),
            dtmc.curr_to_next_map.get(),
        ));

        post.replace(dd::bdd_and_then_existsabs(
            dtmc.get_transitions_01(),
            sol_next.get(),
            dtmc.next_var_set.get(),
        ));

        step.replace(dd::bdd_and(phi1, post.get()));

        sol_prime.replace(dd::bdd_or(sol.get(), step.get()));

        if sol_prime.get() == sol.get() {
            sol.set(sol_prime.get());
            break;
        }
        sol.set(sol_prime.get());
    }

    trace!("prob0 converged in {} iterations", iterations);

    let reachable = dtmc.get_reachable_bdd();
    protected_bdd!(not_sol, dd::bdd_not(sol.get()));
    dd::bdd_and(reachable, not_sol.get())
}

fn prob1(dtmc: &mut SymbolicDTMC, phi1: BddNode, phi2: BddNode, s_no: BddNode) -> BddNode {
    protected_bdd!(not_phi2, dd::bdd_not(phi2));
    protected_bdd!(phi1_and_not_phi2, dd::bdd_and(phi1, not_phi2.get()));

    protected_bdd!(sol, s_no);
    protected_bdd!(sol_next, sol.get());
    protected_bdd!(post, sol.get());
    protected_bdd!(step, sol.get());
    protected_bdd!(sol_prime, sol.get());
    let mut iterations = 0usize;

    loop {
        iterations += 1;

        sol_next.replace(dd::bdd_compose_with_map(
            sol.get(),
            dtmc.curr_to_next_map.get(),
        ));

        post.replace(dd::bdd_and_then_existsabs(
            dtmc.get_transitions_01(),
            sol_next.get(),
            dtmc.next_var_set.get(),
        ));

        step.replace(dd::bdd_and(phi1_and_not_phi2.get(), post.get()));

        sol_prime.replace(dd::bdd_or(sol.get(), step.get()));

        if sol_prime.get() == sol.get() {
            sol.set(sol_prime.get());
            break;
        }
        sol.set(sol_prime.get());
    }

    trace!("prob1 converged in {} iterations", iterations);

    protected_bdd!(not_sol, dd::bdd_not(sol.get()));
    dd::bdd_and(dtmc.get_reachable_bdd(), not_sol.get())
}

fn check_unbounded_until_probability_add(
    dtmc: &mut SymbolicDTMC,
    phi1: &Expr,
    phi2: &Expr,
) -> AddNode {
    info!("Checking unbounded until");
    protected_bdd!(phi1_bdd, state_formula_to_bdd(dtmc, phi1));
    protected_bdd!(phi2_bdd, state_formula_to_bdd(dtmc, phi2));

    protected_bdd!(s_no, prob0(dtmc, phi1_bdd.get(), phi2_bdd.get()));
    protected_bdd!(
        s_yes,
        prob1(dtmc, phi1_bdd.get(), phi2_bdd.get(), s_no.get())
    );

    protected_bdd!(no_or_yes, dd::bdd_or(s_no.get(), s_yes.get()));
    protected_bdd!(not_no_or_yes, dd::bdd_not(no_or_yes.get()));

    protected_bdd!(
        s_question,
        dd::bdd_and(dtmc.get_reachable_bdd(), not_no_or_yes.get())
    );

    protected_add!(s_question_add, dd::bdd_to_add(s_question.get()));

    protected_add!(
        t_question,
        dd::add_times(dtmc.transitions.get(), s_question_add.get())
    );

    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    protected_add!(identity_add, dd::bdd_to_add(identity_bdd));

    protected_add!(a, dd::add_minus(identity_add.get(), t_question.get()));

    protected_add!(b, dd::bdd_to_add(s_yes.get()));

    solve_jacobi(dtmc, a.get(), b.get(), b.get())
}

/// Evaluates one property at the single initial state.
pub fn evaluate_property_at_initial_state(
    dtmc: &mut SymbolicDTMC,
    property: &DTMCProperty,
) -> Result<PropertyEvaluation> {
    match property {
        DTMCProperty::ProbQuery(PathFormula::Next(phi)) => {
            info!("Checking probability next property: {}", property);
            let probability_add = check_next_probability_add(dtmc, phi);
            let value = evaluate_add_in_initial_state(dtmc, probability_add);
            debug!("Computed P=? [X phi] value at initial state: {}", value);
            Ok(PropertyEvaluation::Probability(value))
        }
        DTMCProperty::ProbQuery(PathFormula::Until {
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
        DTMCProperty::ProbQuery(PathFormula::Until {
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
        DTMCProperty::ProbQuery(PathFormula::Release { .. }) => {
            Ok(PropertyEvaluation::Unsupported(
                "Release properties (and G sugar) are not supported yet",
            ))
        }
        DTMCProperty::RewardQuery(_) => Ok(PropertyEvaluation::Unsupported(
            "Reward properties are not supported yet",
        )),
    }
}
