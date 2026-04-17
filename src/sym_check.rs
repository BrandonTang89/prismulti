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
use crate::dd;
use crate::ref_manager::protected_local::{
    ProtectedAddLocal, ProtectedBddLocal, ProtectedMapLocal, ProtectedVarSetLocal,
};
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
    let expr_add = ProtectedAddLocal::new(translate_expr(expr, dtmc));
    dd::add_to_bdd(&mut dtmc.mgr, expr_add.get())
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
    let phi_bdd = ProtectedBddLocal::new(state_formula_to_bdd(dtmc, phi));
    let phi_add = ProtectedAddLocal::new(dd::bdd_to_add(&mut dtmc.mgr, phi_bdd.get()));
    let curr_to_next_swap_map = ProtectedMapLocal::new(dd::get_swap_map_for_indices(
        &mut dtmc.mgr,
        &dtmc.curr_var_indices,
        &dtmc.next_var_indices,
    ));
    let phi_next = ProtectedAddLocal::new(dd::add_compose_with_map(
        &mut dtmc.mgr,
        phi_add.get(),
        curr_to_next_swap_map.get(),
    ));
    let next_var_set = ProtectedVarSetLocal::new(dd::get_var_set_for_indices(
        &dtmc.mgr,
        &dtmc.next_var_indices,
    ));

    dd::add_matrix_multiply_with_var_set(
        &dtmc.mgr,
        dtmc.transitions.get(),
        phi_next.get(),
        next_var_set.get(),
    )
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
    let phi1_bdd = ProtectedBddLocal::new(state_formula_to_bdd(dtmc, phi1));
    let phi2_bdd = ProtectedBddLocal::new(state_formula_to_bdd(dtmc, phi2));

    let s_yes_add = ProtectedAddLocal::new(dd::bdd_to_add(&mut dtmc.mgr, phi2_bdd.get()));

    let not_phi2 = ProtectedBddLocal::new(dd::bdd_not(&dtmc.mgr, phi2_bdd.get()));
    let phi1_and_not_phi2 =
        ProtectedBddLocal::new(dd::bdd_and(&dtmc.mgr, phi1_bdd.get(), not_phi2.get()));

    let reachable = dtmc.get_reachable_bdd();
    let s_question =
        ProtectedBddLocal::new(dd::bdd_and(&dtmc.mgr, reachable, phi1_and_not_phi2.get()));
    let s_question_add = ProtectedAddLocal::new(dd::bdd_to_add(&mut dtmc.mgr, s_question.get()));

    let t_question = ProtectedAddLocal::new(dd::add_times(
        &mut dtmc.mgr,
        s_question_add.get(),
        dtmc.transitions.get(),
    ));
    let curr_to_next_swap_map = ProtectedMapLocal::new(dd::get_swap_map_for_indices(
        &mut dtmc.mgr,
        &dtmc.curr_var_indices,
        &dtmc.next_var_indices,
    ));
    let next_var_set = ProtectedVarSetLocal::new(dd::get_var_set_for_indices(
        &dtmc.mgr,
        &dtmc.next_var_indices,
    ));

    let mut res_add = ProtectedAddLocal::new(s_yes_add.get());
    for i in 1..=k {
        trace!("Bounded-until iteration {}/{}", i, k);
        let renamed = ProtectedAddLocal::new(dd::add_compose_with_map(
            &mut dtmc.mgr,
            res_add.get(),
            curr_to_next_swap_map.get(),
        ));

        let stepped = ProtectedAddLocal::new(dd::add_matrix_multiply_with_var_set(
            &dtmc.mgr,
            t_question.get(),
            renamed.get(),
            next_var_set.get(),
        ));

        res_add.set(dd::add_plus(&mut dtmc.mgr, stepped.get(), s_yes_add.get()));
    }
    res_add.get()
}

/// __Refs__: result\
/// __Derefs__: a, b, init
fn solve_jacobi(dtmc: &mut SymbolicDTMC, a: AddNode, b: AddNode, init: AddNode) -> AddNode {
    let curr_to_next_swap_map = ProtectedMapLocal::new(dd::get_swap_map_for_indices(
        &mut dtmc.mgr,
        &dtmc.curr_var_indices,
        &dtmc.next_var_indices,
    ));
    let next_var_set = ProtectedVarSetLocal::new(dd::get_var_set_for_indices(
        &dtmc.mgr,
        &dtmc.next_var_indices,
    ));

    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    let identity_add = ProtectedAddLocal::new(dd::bdd_to_add(&mut dtmc.mgr, identity_bdd));

    let a_diag = ProtectedAddLocal::new(dd::add_times(&mut dtmc.mgr, a, identity_add.get()));

    let ones = ProtectedAddLocal::new(dd::add_const(&dtmc.mgr, 1.0));
    let d = ProtectedAddLocal::new(dd::add_matrix_multiply_with_var_set(
        &dtmc.mgr,
        a_diag.get(),
        ones.get(),
        next_var_set.get(),
    ));

    let d = ProtectedAddLocal::new(dd::add_max_abstract(
        &dtmc.mgr,
        d.get(),
        dtmc.next_var_cube.get(),
    ));

    let neg_one = ProtectedAddLocal::new(dd::add_const(&dtmc.mgr, -1.0));
    let a_neg = ProtectedAddLocal::new(dd::add_times(&mut dtmc.mgr, a, neg_one.get()));

    let not_identity_bdd = ProtectedBddLocal::new(dd::bdd_not(&dtmc.mgr, identity_bdd));
    let not_identity_add =
        ProtectedAddLocal::new(dd::bdd_to_add(&mut dtmc.mgr, not_identity_bdd.get()));
    let a_off_diag = ProtectedAddLocal::new(dd::add_times(
        &mut dtmc.mgr,
        a_neg.get(),
        not_identity_add.get(),
    ));

    let a_prime = ProtectedAddLocal::new(dd::add_divide(&mut dtmc.mgr, a_off_diag.get(), d.get()));

    let b_prime = ProtectedAddLocal::new(dd::add_divide(&mut dtmc.mgr, b, d.get()));

    let mut sol = ProtectedAddLocal::new(init);
    let mut iterations = 0usize;
    loop {
        iterations += 1;
        let sol_next = ProtectedAddLocal::new(dd::add_compose_with_map(
            &mut dtmc.mgr,
            sol.get(),
            curr_to_next_swap_map.get(),
        ));

        let matmul = ProtectedAddLocal::new(dd::add_matrix_multiply_with_var_set(
            &dtmc.mgr,
            a_prime.get(),
            sol_next.get(),
            next_var_set.get(),
        ));

        let sol_prime =
            ProtectedAddLocal::new(dd::add_plus(&mut dtmc.mgr, matmul.get(), b_prime.get()));

        if dd::add_equal_sup_norm(
            &dtmc.mgr,
            sol.get(),
            sol_prime.get(),
            dd::epsilon(&dtmc.mgr),
        ) {
            info!("Jacobi converged in {} iterations", iterations);
            return sol_prime.get();
        }

        sol.set(sol_prime.get());
    }
}

/// __Refs__: result\
/// __Derefs__: phi1, phi2
fn prob0(dtmc: &mut SymbolicDTMC, phi1: BddNode, phi2: BddNode) -> BddNode {
    let curr_to_next_swap_map = ProtectedMapLocal::new(dd::get_swap_map_for_indices(
        &mut dtmc.mgr,
        &dtmc.curr_var_indices,
        &dtmc.next_var_indices,
    ));

    let mut sol = ProtectedBddLocal::new(phi2);
    let mut iterations = 0usize;

    loop {
        iterations += 1;

        let sol_next = ProtectedBddLocal::new(dd::bdd_compose_with_map(
            &mut dtmc.mgr,
            sol.get(),
            curr_to_next_swap_map.get(),
        ));

        let t_01 = dtmc.get_transitions_01();
        let post = ProtectedBddLocal::new(dd::bdd_and_then_existsabs(
            &dtmc.mgr,
            t_01,
            sol_next.get(),
            dtmc.next_var_cube.get(),
        ));

        let step = ProtectedBddLocal::new(dd::bdd_and(&dtmc.mgr, phi1, post.get()));

        let sol_prime = ProtectedBddLocal::new(dd::bdd_or(&dtmc.mgr, sol.get(), step.get()));

        if sol_prime.get() == sol.get() {
            sol.set(sol_prime.get());
            break;
        }
        sol.set(sol_prime.get());
    }

    trace!("prob0 converged in {} iterations", iterations);

    let reachable = dtmc.get_reachable_bdd();
    let not_sol = ProtectedBddLocal::new(dd::bdd_not(&dtmc.mgr, sol.get()));
    dd::bdd_and(&dtmc.mgr, reachable, not_sol.get())
}

/// __Refs__: result\
/// __Derefs__: phi1, phi2, s_no
fn prob1(dtmc: &mut SymbolicDTMC, phi1: BddNode, phi2: BddNode, s_no: BddNode) -> BddNode {
    let curr_to_next_swap_map = ProtectedMapLocal::new(dd::get_swap_map_for_indices(
        &mut dtmc.mgr,
        &dtmc.curr_var_indices,
        &dtmc.next_var_indices,
    ));

    let not_phi2 = ProtectedBddLocal::new(dd::bdd_not(&dtmc.mgr, phi2));
    let phi1_and_not_phi2 = ProtectedBddLocal::new(dd::bdd_and(&dtmc.mgr, phi1, not_phi2.get()));

    let mut sol = ProtectedBddLocal::new(s_no);
    let mut iterations = 0usize;

    loop {
        iterations += 1;

        let sol_next = ProtectedBddLocal::new(dd::bdd_compose_with_map(
            &mut dtmc.mgr,
            sol.get(),
            curr_to_next_swap_map.get(),
        ));

        let t_01 = dtmc.get_transitions_01();
        let post = ProtectedBddLocal::new(dd::bdd_and_then_existsabs(
            &dtmc.mgr,
            t_01,
            sol_next.get(),
            dtmc.next_var_cube.get(),
        ));

        let step =
            ProtectedBddLocal::new(dd::bdd_and(&dtmc.mgr, phi1_and_not_phi2.get(), post.get()));

        let sol_prime = ProtectedBddLocal::new(dd::bdd_or(&dtmc.mgr, sol.get(), step.get()));

        if sol_prime.get() == sol.get() {
            sol.set(sol_prime.get());
            break;
        }
        sol.set(sol_prime.get());
    }

    trace!("prob1 converged in {} iterations", iterations);

    let reachable = dtmc.get_reachable_bdd();
    let not_sol = ProtectedBddLocal::new(dd::bdd_not(&dtmc.mgr, sol.get()));
    dd::bdd_and(&dtmc.mgr, reachable, not_sol.get())
}

/// __Refs__: result\
/// __Derefs__: none
fn check_unbounded_until_probability_add(
    dtmc: &mut SymbolicDTMC,
    phi1: &Expr,
    phi2: &Expr,
) -> AddNode {
    info!("Checking unbounded until");
    let phi1_bdd = ProtectedBddLocal::new(state_formula_to_bdd(dtmc, phi1));
    let phi2_bdd = ProtectedBddLocal::new(state_formula_to_bdd(dtmc, phi2));

    let s_no = ProtectedBddLocal::new(prob0(dtmc, phi1_bdd.get(), phi2_bdd.get()));
    let s_yes = ProtectedBddLocal::new(prob1(dtmc, phi1_bdd.get(), phi2_bdd.get(), s_no.get()));

    let no_or_yes = ProtectedBddLocal::new(dd::bdd_or(&dtmc.mgr, s_no.get(), s_yes.get()));
    let not_no_or_yes = ProtectedBddLocal::new(dd::bdd_not(&dtmc.mgr, no_or_yes.get()));

    let reachable = dtmc.get_reachable_bdd();
    let s_question = ProtectedBddLocal::new(dd::bdd_and(&dtmc.mgr, reachable, not_no_or_yes.get()));

    let s_question_add = ProtectedAddLocal::new(dd::bdd_to_add(&mut dtmc.mgr, s_question.get()));

    let t_question = ProtectedAddLocal::new(dd::add_times(
        &mut dtmc.mgr,
        dtmc.transitions.get(),
        s_question_add.get(),
    ));

    let identity_bdd = dtmc.get_curr_next_identity_bdd();
    let identity_add = ProtectedAddLocal::new(dd::bdd_to_add(&mut dtmc.mgr, identity_bdd));

    let a = ProtectedAddLocal::new(dd::add_minus(
        &mut dtmc.mgr,
        identity_add.get(),
        t_question.get(),
    ));

    let b = ProtectedAddLocal::new(dd::bdd_to_add(&mut dtmc.mgr, s_yes.get()));

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
