use core::num;
use std::collections::{HashMap, HashSet};

use sylvan_sys::{BDD, BDDVAR};

#[allow(unused_imports)]
use tracing::{debug, info, trace};

use crate::analyze::DTMCModelInfo;
use crate::ast::*;
use crate::reachability::compute_reachable_and_filter;
use crate::ref_manager::local_roots_guard::LocalRootsGuard;
use crate::ref_manager::protected_slot::{ProtectedAddSlot, ProtectedBddSlot};
use crate::ref_manager::{AddNode, BddNode};
use crate::symbolic_dtmc::SymbolicDTMC;

/// Internal symbolic representation of a single command.
#[derive(Debug)]
struct SymbolicCommand {
    /// Referenced ADD for `guard * sum(prob_i * assignment_i)`.
    transition: ProtectedAddSlot,
}

/// Internal symbolic representation of a module.
#[derive(Debug)]
struct SymbolicModule {
    /// Referenced ADD identity relation for this module (`x' = x` for all locals).
    ident: ProtectedAddSlot,
    /// Referenced command transitions grouped by action label.
    commands_by_action: HashMap<String, Vec<SymbolicCommand>>,
}

/// Allocate DD variables for every model variable and build current/next cubes.
fn allocate_dd_vars(dtmc: &mut SymbolicDTMC) {
    for module in &dtmc.ast.modules {
        for var_decl in &module.local_vars {
            let var_name = &var_decl.name;
            let num_bits = match &var_decl.var_type {
                VarType::Bool => 1,
                VarType::BoundedInt { lo, hi } => {
                    let lo_val = match lo.as_ref() {
                        Expr::IntLit(val) => *val,
                        _ => panic!("Expected integer literal for variable bounds"),
                    };
                    let hi_val = match hi.as_ref() {
                        Expr::IntLit(val) => *val,
                        _ => panic!("Expected integer literal for variable bounds"),
                    };
                    let range_size = hi_val - lo_val + 1;
                    match range_size {
                        0 => panic!("Invalid variable bounds: lo must be <= hi"),
                        1 => panic!("Variable '{}' has only one possible value", var_name),
                        _ => (range_size - 1).ilog2() + 1,
                    }
                }
            };

            let mgr = &mut dtmc.mgr;

            let mut curr_indices = Vec::with_capacity(num_bits as usize);
            let mut next_indices = Vec::with_capacity(num_bits as usize);
            for _ in 0..num_bits {
                let curr = mgr.new_var();
                let next = mgr.new_var();

                curr_indices.push(curr);
                next_indices.push(next);

                let curr_node = mgr.bdd_var(curr);
                dtmc.var_node_roots.push(ProtectedBddSlot::new(curr_node));
                let next_node = mgr.bdd_var(next);
                dtmc.var_node_roots.push(ProtectedBddSlot::new(next_node));
            }

            for (i, &curr) in curr_indices.iter().enumerate() {
                dtmc.dd_var_names
                    .insert(curr, format!("{}_{}", var_name, i));
            }
            for (i, &next) in next_indices.iter().enumerate() {
                dtmc.dd_var_names
                    .insert(next, format!("{}'_{}", var_name, i));
            }

            dtmc.curr_name_to_indices
                .insert(var_name.clone(), curr_indices);
            dtmc.next_name_to_indices
                .insert(var_name.clone(), next_indices);

            trace!(
                "Allocated var '{}' with curr BDD vars: {:?}",
                var_name,
                dtmc.curr_name_to_indices[var_name]
            );
            trace!(
                "Allocated var '{}' with next BDD vars: {:?}",
                var_name,
                dtmc.next_name_to_indices[var_name]
            );
        }
    }

    let mut curr_var_indices = Vec::new();
    let mut next_var_indices = Vec::new();
    for module in &dtmc.ast.modules {
        for var_decl in &module.local_vars {
            let var_name = &var_decl.name;
            for (&curr, &next) in dtmc.curr_name_to_indices[var_name]
                .iter()
                .zip(dtmc.next_name_to_indices[var_name].iter())
            {
                curr_var_indices.push(curr);
                next_var_indices.push(next);
            }
        }
    }
    dtmc.curr_var_indices = curr_var_indices;
    dtmc.next_var_indices = next_var_indices;

    let curr_var_set = dtmc.mgr.var_set_from_indices(&dtmc.curr_var_indices);
    dtmc.curr_var_set.set(curr_var_set);
    let next_var_set = dtmc.mgr.var_set_from_indices(&dtmc.next_var_indices);
    dtmc.next_var_cube.set(next_var_set);
}

/// Return ADD encoding of variable value (`curr` or `next`) with lower-bound offset.
fn get_variable_encoding(dtmc: &mut SymbolicDTMC, var_name: &str, primed: bool) -> AddNode {
    let (lo, _) = dtmc
        .info
        .var_bounds
        .get(var_name)
        .unwrap_or_else(|| panic!("Variable '{}' not found in model info", var_name));

    let mgr = &mut dtmc.mgr;
    let offset_add = mgr.add_const(*lo as f64);
    let variable_nodes = if primed {
        &dtmc.next_name_to_indices[var_name]
    } else {
        &dtmc.curr_name_to_indices[var_name]
    };
    let encoding = mgr.get_encoding(variable_nodes);
    mgr.add_plus(encoding, offset_add)
}

/// Translate an AST expression to a referenced ADD node.
///
/// This is shared by symbolic construction and symbolic property checking to
/// keep state-formula semantics consistent.
pub fn translate_expr(expr: &Expr, dtmc: &mut SymbolicDTMC) -> AddNode {
    match expr {
        Expr::IntLit(i) => dtmc.mgr.add_const(*i as f64),
        Expr::FloatLit(f) => dtmc.mgr.add_const(*f),
        Expr::BoolLit(b) => dtmc.mgr.add_const(if *b { 1.0 } else { 0.0 }),
        Expr::Ident(name) => get_variable_encoding(dtmc, name, false),
        Expr::PrimedIdent(name) => get_variable_encoding(dtmc, name, true),
        Expr::LabelRef(name) => {
            panic!(
                "Unresolved label reference should not reach symbolic translation: \"{}\"",
                name
            )
        }
        Expr::UnaryOp { op, operand } => {
            let mut guard = LocalRootsGuard::new();
            crate::new_protected!(guard, value, translate_expr(operand, dtmc));
            match op {
                UnOp::Not => {
                    crate::new_protected!(guard, one, dtmc.mgr.add_const(1.0));
                    dtmc.mgr.add_minus(one, value)
                }
                UnOp::Neg => {
                    crate::new_protected!(guard, zero, dtmc.mgr.add_const(0.0));
                    dtmc.mgr.add_minus(zero, value)
                }
            }
        }
        Expr::BinOp { lhs, op, rhs } => {
            let mut guard = LocalRootsGuard::new();
            crate::new_protected!(guard, left, translate_expr(lhs, dtmc));
            crate::new_protected!(guard, right, translate_expr(rhs, dtmc));
            match op {
                BinOp::Plus => dtmc.mgr.add_plus(left, right),
                BinOp::Minus => dtmc.mgr.add_minus(left, right),
                BinOp::Mul => dtmc.mgr.add_times(left, right),
                BinOp::Div => dtmc.mgr.add_divide(left, right),
                BinOp::Eq => {
                    let bdd = dtmc.mgr.add_equals(left, right);
                    dtmc.mgr.bdd_to_add(bdd)
                }
                BinOp::Neq => {
                    let bdd = dtmc.mgr.add_nequals(left, right);
                    dtmc.mgr.bdd_to_add(bdd)
                }
                BinOp::Lt => {
                    let bdd = dtmc.mgr.add_less_than(left, right);
                    dtmc.mgr.bdd_to_add(bdd)
                }
                BinOp::Leq => {
                    let bdd = dtmc.mgr.add_less_or_equal(left, right);
                    dtmc.mgr.bdd_to_add(bdd)
                }
                BinOp::Gt => {
                    let bdd = dtmc.mgr.add_greater_than(left, right);
                    dtmc.mgr.bdd_to_add(bdd)
                }
                BinOp::Geq => {
                    let bdd = dtmc.mgr.add_greater_or_equal(left, right);
                    dtmc.mgr.bdd_to_add(bdd)
                }
                BinOp::And => dtmc.mgr.add_times(left, right),
                BinOp::Or => {
                    let add01_left = dtmc.mgr.add_to_bdd(left);
                    let add01_right = dtmc.mgr.add_to_bdd(right);
                    let bdd_or = dtmc.mgr.bdd_or(add01_left, add01_right);
                    dtmc.mgr.bdd_to_add(bdd_or)
                }
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let mut guard = LocalRootsGuard::new();
            crate::new_protected!(guard, cond_expr, translate_expr(cond, dtmc));
            crate::new_protected!(guard, cond_add, dtmc.mgr.add_to_bdd(cond_expr));
            crate::new_protected!(guard, then_add, translate_expr(then_branch, dtmc));
            crate::new_protected!(guard, else_add, translate_expr(else_branch, dtmc));
            dtmc.mgr.add_ite(cond_add, then_add, else_add)
        }
    }
}

/// If expression is of the form `(x' = ...)`, return `x`.
fn get_assign_target(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::BinOp {
            lhs, op: BinOp::Eq, ..
        } => match &**lhs {
            Expr::PrimedIdent(name) => Some(name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// Translate one probabilistic update branch.
///
/// Adds stutter constraints for module-local variables that are not explicitly
/// assigned in the branch.
fn translate_update(
    update: &ProbUpdate,
    module_local_vars: &[String],
    dtmc: &mut SymbolicDTMC,
) -> AddNode {
    let mut guard = LocalRootsGuard::new();
    crate::new_protected!(guard, prob, translate_expr(&update.prob, dtmc));

    let assigned_vars: HashSet<String> = update
        .assignments
        .iter()
        .filter_map(|assignment| get_assign_target(assignment).map(|name| name.to_string()))
        .collect();

    let mut symbolic_updates: Vec<AddNode> = update
        .assignments
        .iter()
        .map(|assignment| translate_expr(assignment, dtmc))
        .collect::<Vec<_>>();
    for symbolic_update in &mut symbolic_updates {
        guard.protect(symbolic_update);
    }

    let mgr = &mut dtmc.mgr;
    let add_one = mgr.add_const(1.0);
    let mut assign = symbolic_updates
        .iter()
        .fold(add_one, |acc, &result| mgr.add_times(acc, result));
    guard.protect(&mut assign);

    for var_name in module_local_vars {
        if assigned_vars.contains(var_name) {
            continue;
        }
        let curr_nodes = dtmc.curr_name_to_indices[var_name].clone();
        let next_nodes = dtmc.next_name_to_indices[var_name].clone();
        for (curr, next) in curr_nodes.into_iter().zip(next_nodes.into_iter()) {
            let eq = mgr.bdd_equals(mgr.bdd_var(curr), mgr.bdd_var(next));
            let eq_add = mgr.bdd_to_add(eq);
            assign = mgr.add_times(assign, eq_add);
        }
    }

    dtmc.mgr.add_times(prob, assign)
}

/// Translate one command: `guard * (sum updates)`.
fn translate_command(
    cmd: &Command,
    module_local_vars: &[String],
    dtmc: &mut SymbolicDTMC,
) -> SymbolicCommand {
    let mut guard = LocalRootsGuard::new();
    crate::new_protected!(guard, cmd_guard, translate_expr(&cmd.guard, dtmc));
    let mut updates = cmd
        .updates
        .iter()
        .map(|update| translate_update(update, module_local_vars, dtmc))
        .collect::<Vec<_>>();
    for update in &mut updates {
        guard.protect(update);
    }

    let mgr = &mut dtmc.mgr;
    let mut updates_sum = updates
        .iter()
        .fold(mgr.add_zero(), |acc, &update| mgr.add_plus(acc, update));
    guard.protect(&mut updates_sum);
    let transition = mgr.add_times(cmd_guard, updates_sum);
    SymbolicCommand {
        transition: ProtectedAddSlot::new(transition),
    }
}

/// Translate one module into identity and per-action command transitions.
fn translate_module(module: &Module, dtmc: &mut SymbolicDTMC) -> SymbolicModule {
    let mut guard = LocalRootsGuard::new();
    let module_local_vars = module
        .local_vars
        .iter()
        .map(|v| v.name.clone())
        .collect::<Vec<_>>();

    crate::new_protected!(guard, ident, dtmc.mgr.bdd_one());
    for var_name in module.local_vars.iter().map(|v| &v.name) {
        let curr_nodes = dtmc.curr_name_to_indices[var_name].clone();
        let next_nodes = dtmc.next_name_to_indices[var_name].clone();
        for (curr, next) in curr_nodes.into_iter().zip(next_nodes.into_iter()) {
            let eq = dtmc
                .mgr
                .bdd_equals(dtmc.mgr.bdd_var(curr), dtmc.mgr.bdd_var(next));
            ident = dtmc.mgr.bdd_and(ident, eq);
        }
    }
    let ident = dtmc.mgr.bdd_to_add(ident);

    let mut commands_by_action: HashMap<String, Vec<SymbolicCommand>> = HashMap::new();
    for cmd in &module.commands {
        let symbolic_cmd = translate_command(cmd, &module_local_vars, dtmc);
        assert!(
            cmd.labels.len() == 1,
            "DTMCs should have exactly one label per command after analysis"
        );
        commands_by_action
            .entry(cmd.labels[0].clone())
            .or_default()
            .push(symbolic_cmd);
    }

    SymbolicModule {
        ident: ProtectedAddSlot::new(ident),
        commands_by_action,
    }
}

/// Translate every module to symbolic form.
fn translate_modules(dtmc: &mut SymbolicDTMC) -> HashMap<String, SymbolicModule> {
    let modules = dtmc.ast.modules.clone();
    modules
        .iter()
        .map(|module| (module.name.clone(), translate_module(module, dtmc)))
        .collect()
}

/// Build and normalize the global DTMC transition ADD.
fn translate_dtmc(dtmc: &mut SymbolicDTMC) {
    let mut guard = LocalRootsGuard::new();
    let symbolic_modules = translate_modules(dtmc);

    crate::new_protected!(guard, transitions, dtmc.mgr.add_zero());
    for (act, act_modules) in &dtmc.info.modules_of_act {
        let mut action_guard = LocalRootsGuard::new();
        trace!("Action '{}' is part of {:?}", act, act_modules);
        crate::new_protected!(action_guard, act_trans, dtmc.mgr.add_const(1.0));

        for module_name in dtmc.ast.modules.iter().map(|m| &m.name) {
            if act_modules.contains(module_name) {
                let mut module_guard = LocalRootsGuard::new();
                crate::new_protected!(module_guard, act_mod_trans, dtmc.mgr.add_zero());
                for cmd in &symbolic_modules[module_name].commands_by_action[act] {
                    act_mod_trans = dtmc.mgr.add_plus(act_mod_trans, cmd.transition.get());
                }
                act_trans = dtmc.mgr.add_times(act_trans, act_mod_trans);
            } else {
                act_trans = dtmc
                    .mgr
                    .add_times(act_trans, symbolic_modules[module_name].ident.get());
            }
        }

        transitions = dtmc.mgr.add_plus(transitions, act_trans);
    }

    transitions = dtmc.mgr.unif(transitions, dtmc.next_var_cube.get());
    dtmc.transitions.replace(transitions);
}

/// Top-level symbolic DTMC construction pipeline.
pub fn build_symbolic_dtmc(ast: DTMCAst, model_info: DTMCModelInfo) -> SymbolicDTMC {
    let mut dtmc = SymbolicDTMC::new(ast, model_info);
    allocate_dd_vars(&mut dtmc);
    translate_dtmc(&mut dtmc);
    info!("Constructed transition ADD");
    compute_reachable_and_filter(&mut dtmc);
    dtmc
}
