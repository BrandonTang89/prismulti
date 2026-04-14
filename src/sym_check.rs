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
use crate::ref_manager::{AddNode, BddNode};
use crate::symbolic_dtmc::SymbolicDTMC;

#[derive(Clone, Debug)]
pub enum PropertyEvaluation {
    Probability(f64),
    Unsupported(&'static str),
}

/// Converts a boolean state formula into a current-state BDD.
fn state_formula_to_bdd(dtmc: &mut SymbolicDTMC, expr: &Expr) -> BddNode {
    trace!("Translating state formula to BDD: {}", expr);
    let expr_add = translate_expr(expr, dtmc);
    dtmc.mgr.add_to_bdd(expr_add)
}

/// Evaluates an ADD of state values at the initial state using `Cudd_Eval`.
fn evaluate_add_in_initial_state(dtmc: &mut SymbolicDTMC, values: AddNode) -> f64 {
    let init = dtmc.get_init_bdd();
    let inputs = dtmc
        .mgr
        .extract_leftmost_path_from_bdd(init)
        .expect("initial-state BDD must be satisfiable");
    dtmc.mgr.deref_node(init.0);

    let out = dtmc.mgr.add_eval_value(values, &inputs);
    dtmc.mgr.deref_node(values.0);
    out
}

/// Computes the probability ADD for `P=? [X phi]`.
fn check_next_probability_add(dtmc: &mut SymbolicDTMC, phi: &Expr) -> AddNode {
    let phi_bdd = state_formula_to_bdd(dtmc, phi);
    let phi_add = dtmc.mgr.bdd_to_add(phi_bdd);
    let phi_next = dtmc
        .mgr
        .add_swap_vars(phi_add, &dtmc.curr_var_indices, &dtmc.next_var_indices);

    dtmc.mgr.ref_node(dtmc.transitions.0);
    dtmc.mgr
        .add_matrix_multiply(dtmc.transitions, phi_next, &dtmc.next_var_indices)
}

/// Computes the probability ADD for `P=? [phi1 U<=k phi2]`.
fn check_bounded_until_probability_add(
    dtmc: &mut SymbolicDTMC,
    phi1: &Expr,
    phi2: &Expr,
    k: u32,
) -> AddNode {
    info!("Checking bounded until with bound k={}", k);

    let phi1_bdd = state_formula_to_bdd(dtmc, phi1);
    let phi2_bdd = state_formula_to_bdd(dtmc, phi2);

    dtmc.mgr.ref_node(phi2_bdd.0);
    let s_yes_add = dtmc.mgr.bdd_to_add(phi2_bdd);

    let not_phi2 = dtmc.mgr.bdd_not(phi2_bdd);
    let phi1_and_not_phi2 = dtmc.mgr.bdd_and(phi1_bdd, not_phi2);

    let reachable = dtmc.get_reachable_bdd();
    let s_question = dtmc.mgr.bdd_and(reachable, phi1_and_not_phi2);
    let s_question_add = dtmc.mgr.bdd_to_add(s_question);

    dtmc.mgr.ref_node(dtmc.transitions.0);
    let t_question = dtmc.mgr.add_times(s_question_add, dtmc.transitions);

    dtmc.mgr.ref_node(s_yes_add.0);
    let mut res_add = AddNode(s_yes_add.0);
    for i in 1..=k {
        trace!("Bounded-until iteration {}/{}", i, k);
        let renamed =
            dtmc.mgr
                .add_swap_vars(res_add, &dtmc.curr_var_indices, &dtmc.next_var_indices);

        dtmc.mgr.ref_node(t_question.0);
        let stepped = dtmc
            .mgr
            .add_matrix_multiply(t_question, renamed, &dtmc.next_var_indices);

        dtmc.mgr.ref_node(s_yes_add.0);
        let s_yes_term = AddNode(s_yes_add.0);
        res_add = dtmc.mgr.add_plus(stepped, s_yes_term);
    }

    dtmc.mgr.deref_node(s_yes_add.0);
    dtmc.mgr.deref_node(t_question.0);
    res_add
}

/// __Refs__: result\
/// __Derefs__: a, b, init
fn solve_jacobi(dtmc: &mut SymbolicDTMC, a: AddNode, b: AddNode, init: AddNode) -> AddNode {
    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    let identity_add = dtmc.mgr.bdd_to_add(identity_bdd);

    dtmc.mgr.ref_node(a.0);
    let a_diag = dtmc.mgr.add_times(a, identity_add);

    let ones = dtmc.mgr.add_const(1.0);
    let d = dtmc
        .mgr
        .add_matrix_multiply(a_diag, ones, &dtmc.next_var_indices);

    dtmc.mgr.ref_node(dtmc.next_var_cube.0);
    let next_var_cube_add = dtmc.mgr.bdd_to_add(dtmc.next_var_cube);
    let d = dtmc.mgr.add_max_abstract(d, next_var_cube_add);
    dtmc.mgr.deref_node(next_var_cube_add.0);

    dtmc.mgr.ref_node(a.0);
    let neg_one = dtmc.mgr.add_const(-1.0);
    let a_neg = dtmc.mgr.add_times(a, neg_one);

    dtmc.mgr.ref_node(identity_bdd.0);
    let not_identity_bdd = dtmc.mgr.bdd_not(identity_bdd);
    let not_identity_add = dtmc.mgr.bdd_to_add(not_identity_bdd);
    let a_off_diag = dtmc.mgr.add_times(a_neg, not_identity_add);

    dtmc.mgr.ref_node(d.0);
    let a_prime = dtmc.mgr.add_divide(a_off_diag, d);

    dtmc.mgr.ref_node(d.0);
    let b_prime = dtmc.mgr.add_divide(b, d);

    dtmc.mgr.deref_node(d.0);
    dtmc.mgr.deref_node(a.0);

    let mut sol = init;
    let mut iterations = 0usize;
    loop {
        iterations += 1;
        dtmc.mgr.ref_node(sol.0);
        let sol_next = dtmc
            .mgr
            .add_swap_vars(sol, &dtmc.curr_var_indices, &dtmc.next_var_indices);

        dtmc.mgr.ref_node(a_prime.0);
        let matmul = dtmc
            .mgr
            .add_matrix_multiply(a_prime, sol_next, &dtmc.next_var_indices);

        dtmc.mgr.ref_node(b_prime.0);
        let sol_prime = dtmc.mgr.add_plus(matmul, b_prime);

        if dtmc
            .mgr
            .add_equal_sup_norm(sol, sol_prime, dtmc.mgr.epsilon())
        {
            dtmc.mgr.deref_node(sol.0);
            dtmc.mgr.deref_node(a_prime.0);
            dtmc.mgr.deref_node(b_prime.0);
            info!("Jacobi converged in {} iterations", iterations);
            return sol_prime;
        }

        dtmc.mgr.deref_node(sol.0);
        sol = sol_prime;
    }
}

fn check_unbounded_until_probability_add(
    dtmc: &mut SymbolicDTMC,
    phi1: &Expr,
    phi2: &Expr,
) -> AddNode {
    info!("Checking unbounded until");

    let phi1_bdd = state_formula_to_bdd(dtmc, phi1);
    let phi2_bdd = state_formula_to_bdd(dtmc, phi2);

    let not_phi1 = dtmc.mgr.bdd_not(phi1_bdd);
    dtmc.mgr.ref_node(phi2_bdd.0);
    let not_phi2 = dtmc.mgr.bdd_not(phi2_bdd);
    let s_no = dtmc.mgr.bdd_and(not_phi1, not_phi2);

    let s_yes = phi2_bdd;

    dtmc.mgr.ref_node(s_yes.0);
    let no_or_yes = dtmc.mgr.bdd_or(s_no, s_yes);
    let not_no_or_yes = dtmc.mgr.bdd_not(no_or_yes);

    let reachable = dtmc.get_reachable_bdd();
    let s_question = dtmc.mgr.bdd_and(reachable, not_no_or_yes);

    let s_question_add = dtmc.mgr.bdd_to_add(s_question);

    dtmc.mgr.ref_node(dtmc.transitions.0);
    let t_question = dtmc.mgr.add_times(dtmc.transitions, s_question_add);

    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    let identity_add = dtmc.mgr.bdd_to_add(identity_bdd);

    let a = dtmc.mgr.add_minus(identity_add, t_question);

    let b = dtmc.mgr.bdd_to_add(s_yes);

    dtmc.mgr.ref_node(b.0);
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
