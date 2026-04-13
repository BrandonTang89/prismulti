//! Symbolic probabilistic model checking for supported DTMC path properties.
//!
//! Currently supported:
//! - `P=? [X phi]`
//! - `P=? [phi1 U<=k phi2]`
//!
//! This module computes an ADD that maps each current state to its probability,
//! then evaluates that ADD in the initial state.

use anyhow::{bail, Result};
use tracing::{debug, info, trace};

use crate::ast::{Expr, PathFormula, Property, VarDecl, VarType};
use crate::constr_symbolic::translate_expr;
use crate::ref_manager::{AddNode, BddNode};
use crate::symbolic_dtmc::SymbolicDTMC;

#[derive(Clone, Debug)]
pub enum PropertyEvaluation {
    Probability(f64),
    Unsupported(&'static str),
}

fn init_value(var_decl: &VarDecl) -> i32 {
    match (&var_decl.var_type, &*var_decl.init) {
        (VarType::BoundedInt { .. }, Expr::IntLit(v)) => *v,
        (VarType::Bool, Expr::BoolLit(b)) => {
            if *b {
                1
            } else {
                0
            }
        }
        (VarType::Bool, Expr::IntLit(v)) if *v == 0 || *v == 1 => *v,
        _ => panic!(
            "Unsupported init expression for variable '{}': {:?}",
            var_decl.name, var_decl.init
        ),
    }
}

fn build_init_bdd(dtmc: &mut SymbolicDTMC) -> BddNode {
    let mut init = dtmc.mgr.bdd_one();

    for module in &dtmc.ast.modules {
        for var_decl in &module.local_vars {
            let var_name = var_decl.name.clone();
            let (lo, hi) = dtmc.info.var_bounds[&var_name];
            let init_val = init_value(var_decl);
            assert!(
                init_val >= lo && init_val <= hi,
                "Initial value of '{}' out of bounds: {} not in [{}..{}]",
                var_name,
                init_val,
                lo,
                hi
            );

            let encoded = (init_val - lo) as u32;
            let curr_nodes = dtmc.var_curr_nodes[&var_name].clone();
            for (i, bit) in curr_nodes.into_iter().enumerate() {
                dtmc.mgr.ref_node(bit);
                let lit = if (encoded & (1u32 << i)) != 0 {
                    BddNode(bit)
                } else {
                    dtmc.mgr.bdd_not(BddNode(bit))
                };
                init = dtmc.mgr.bdd_and(init, lit);
            }
        }
    }

    init
}

fn curr_next_var_indices(dtmc: &SymbolicDTMC) -> (Vec<u16>, Vec<u16>) {
    let mut curr_indices = Vec::new();
    let mut next_indices = Vec::new();

    for module in &dtmc.ast.modules {
        for var_decl in &module.local_vars {
            let var_name = &var_decl.name;
            let curr_nodes = &dtmc.var_curr_nodes[var_name];
            let next_nodes = &dtmc.var_next_nodes[var_name];
            for (&curr, &next) in curr_nodes.iter().zip(next_nodes.iter()) {
                curr_indices.push(dtmc.mgr.read_var_index(curr));
                next_indices.push(dtmc.mgr.read_var_index(next));
            }
        }
    }

    (curr_indices, next_indices)
}

fn state_formula_to_bdd(dtmc: &mut SymbolicDTMC, expr: &Expr) -> BddNode {
    trace!("Translating state formula to BDD: {}", expr);
    let expr_add = translate_expr(expr, dtmc);
    dtmc.mgr.add_to_bdd(expr_add)
}

fn rename_curr_to_next_add(dtmc: &mut SymbolicDTMC, add: AddNode) -> AddNode {
    let (curr_indices, next_indices) = curr_next_var_indices(dtmc);
    dtmc.mgr
        .add_swap_variables(add, &curr_indices, &next_indices)
}

fn evaluate_add_in_initial_state(dtmc: &mut SymbolicDTMC, values: AddNode) -> f64 {
    let init = build_init_bdd(dtmc);
    let init_add = dtmc.mgr.bdd_to_add(init);
    let masked = dtmc.mgr.add_times(values, init_add);

    dtmc.mgr.ref_node(dtmc.curr_var_cube.0);
    let curr_cube_add = dtmc.mgr.bdd_to_add(dtmc.curr_var_cube);
    let sum = dtmc.mgr.add_sum_abstract(masked, curr_cube_add);
    dtmc.mgr.deref_node(curr_cube_add.0);
    let out = dtmc.mgr.add_value(sum.0).unwrap_or(0.0);
    dtmc.mgr.deref_node(sum.0);
    out
}

fn check_next_probability_add(dtmc: &mut SymbolicDTMC, phi: &Expr) -> AddNode {
    let (_, next_indices) = curr_next_var_indices(dtmc);
    let phi_bdd = state_formula_to_bdd(dtmc, phi);
    let phi_add = dtmc.mgr.bdd_to_add(phi_bdd);
    let phi_next = rename_curr_to_next_add(dtmc, phi_add);

    dtmc.mgr.ref_node(dtmc.transitions.0);
    dtmc.mgr
        .add_matrix_multiply(dtmc.transitions, phi_next, &next_indices)
}

fn check_bounded_until_probability_add(
    dtmc: &mut SymbolicDTMC,
    phi1: &Expr,
    phi2: &Expr,
    k: u32,
) -> AddNode {
    info!("Checking bounded until with bound k={}", k);
    let (_, next_indices) = curr_next_var_indices(dtmc);

    let phi1_bdd = state_formula_to_bdd(dtmc, phi1);
    let phi2_bdd = state_formula_to_bdd(dtmc, phi2);

    dtmc.mgr.ref_node(phi2_bdd.0);
    let s_yes_add = dtmc.mgr.bdd_to_add(phi2_bdd);

    // Unknown states are those that are reachable, satisfy phi1, and do not yet
    // satisfy phi2. States outside this set contribute only through the s_yes term.
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
        let renamed = rename_curr_to_next_add(dtmc, res_add);

        dtmc.mgr.ref_node(t_question.0);
        let stepped = dtmc
            .mgr
            .add_matrix_multiply(t_question, renamed, &next_indices);

        dtmc.mgr.ref_node(s_yes_add.0);
        let s_yes_term = AddNode(s_yes_add.0);
        res_add = dtmc.mgr.add_plus(stepped, s_yes_term);
    }

    dtmc.mgr.deref_node(s_yes_add.0);
    dtmc.mgr.deref_node(t_question.0);
    res_add
}

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
