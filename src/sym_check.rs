//! Symbolic probabilistic model checking for supported DTMC path properties.
//!
//! Currently supported:
//! - `P=? [X phi]`
//! - `P=? [phi1 U<=k phi2]`
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
    dtmc.mgr.ref_node(dtmc.init.0);
    let init = dtmc.init;
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

    dtmc.mgr.ref_node(dtmc.reachable.0);
    let s_question = dtmc.mgr.bdd_and(dtmc.reachable, phi1_and_not_phi2);
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
        Property::ProbQuery(PathFormula::Until { bound: None, .. }) => Ok(
            PropertyEvaluation::Unsupported("Unbounded until is not supported yet"),
        ),
        Property::RewardQuery(_) => Ok(PropertyEvaluation::Unsupported(
            "Reward properties are not supported yet",
        )),
    }
}
