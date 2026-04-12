use std::collections::{HashMap, HashSet};

#[allow(unused_imports)]
use tracing::{debug, trace};

use crate::analyze::DTMCModelInfo;
use crate::ast::*;
use crate::reachability::compute_reachable_and_filter;
use crate::ref_manager::{Add01Node, AddNode, NodeId};
use crate::symbolic_dtmc::SymbolicDTMC;

/// Internal symbolic representation of a single command.
#[derive(Debug)]
struct SymbolicCommand {
    /// Referenced ADD for `guard * sum(prob_i * assignment_i)`.
    transition: AddNode,
}

/// Internal symbolic representation of a module.
#[derive(Debug)]
struct SymbolicModule {
    /// Referenced ADD identity relation for this module (`x' = x` for all locals).
    ident: AddNode,
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
            let nodes: Vec<NodeId> = (0..num_bits * 2).map(|_| mgr.new_var().0).collect();
            let curr_nodes: Vec<NodeId> = nodes.chunks(2).map(|c| c[0]).collect();
            let next_nodes: Vec<NodeId> = nodes.chunks(2).map(|c| c[1]).collect();

            for (i, &curr) in curr_nodes.iter().enumerate() {
                dtmc.dd_var_names
                    .insert(curr, format!("{}_{}", var_name, i));
            }
            for (i, &next) in next_nodes.iter().enumerate() {
                dtmc.dd_var_names
                    .insert(next, format!("{}'_{}", var_name, i));
            }

            dtmc.curr_var_cube = curr_nodes.iter().fold(dtmc.curr_var_cube, |cube, &node| {
                mgr.ref_node(node);
                mgr.add01_and(cube, Add01Node(node))
            });
            dtmc.next_var_cube = next_nodes.iter().fold(dtmc.next_var_cube, |cube, &node| {
                mgr.ref_node(node);
                mgr.add01_and(cube, Add01Node(node))
            });

            dtmc.var_curr_nodes.insert(var_name.clone(), curr_nodes);
            dtmc.var_next_nodes.insert(var_name.clone(), next_nodes);

            trace!(
                "Allocated var '{}' with curr BDD vars: {:?}",
                var_name,
                dtmc.var_curr_nodes[var_name]
            );
            trace!(
                "Allocated var '{}' with next BDD vars: {:?}",
                var_name,
                dtmc.var_next_nodes[var_name]
            );
        }
    }
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
        &dtmc.var_next_nodes[var_name]
    } else {
        &dtmc.var_curr_nodes[var_name]
    };
    let encoding = mgr.get_encoding(variable_nodes);
    mgr.add_plus(encoding, offset_add)
}

/// Translate an AST expression to a referenced ADD node.
fn translate_expr(expr: &Expr, dtmc: &mut SymbolicDTMC) -> AddNode {
    match expr {
        Expr::IntLit(i) => dtmc.mgr.add_const(*i as f64),
        Expr::FloatLit(f) => dtmc.mgr.add_const(*f),
        Expr::BoolLit(b) => dtmc.mgr.add_const(if *b { 1.0 } else { 0.0 }),
        Expr::Ident(name) => get_variable_encoding(dtmc, name, false),
        Expr::PrimedIdent(name) => get_variable_encoding(dtmc, name, true),
        Expr::UnaryOp { op, operand } => {
            let value = translate_expr(operand, dtmc);
            match op {
                UnOp::Not => {
                    let one = dtmc.mgr.add_const(1.0);
                    dtmc.mgr.add_minus(one, value)
                }
                UnOp::Neg => {
                    let zero = dtmc.mgr.add_const(0.0);
                    dtmc.mgr.add_minus(zero, value)
                }
            }
        }
        Expr::BinOp { lhs, op, rhs } => {
            let left = translate_expr(lhs, dtmc);
            let right = translate_expr(rhs, dtmc);
            match op {
                BinOp::Plus => dtmc.mgr.add_plus(left, right),
                BinOp::Minus => dtmc.mgr.add_minus(left, right),
                BinOp::Mul => dtmc.mgr.add_times(left, right),
                BinOp::Div => dtmc.mgr.add_divide(left, right),
                BinOp::Eq => {
                    let bdd = dtmc.mgr.add_equals(left, right);
                    dtmc.mgr.add01_to_add(bdd)
                }
                BinOp::Neq => {
                    let bdd = dtmc.mgr.add_nequals(left, right);
                    dtmc.mgr.add01_to_add(bdd)
                }
                BinOp::Lt => {
                    let bdd = dtmc.mgr.add_less_than(left, right);
                    dtmc.mgr.add01_to_add(bdd)
                }
                BinOp::Leq => {
                    let bdd = dtmc.mgr.add_less_or_equal(left, right);
                    dtmc.mgr.add01_to_add(bdd)
                }
                BinOp::Gt => {
                    let bdd = dtmc.mgr.add_greater_than(left, right);
                    dtmc.mgr.add01_to_add(bdd)
                }
                BinOp::Geq => {
                    let bdd = dtmc.mgr.add_greater_or_equal(left, right);
                    dtmc.mgr.add01_to_add(bdd)
                }
                BinOp::And => dtmc.mgr.add_times(left, right),
                BinOp::Or => {
                    let add01_left = dtmc.mgr.add01_from_add(left);
                    let add01_right = dtmc.mgr.add01_from_add(right);
                    let add01_or = dtmc.mgr.add01_or(add01_left, add01_right);
                    dtmc.mgr.add01_to_add(add01_or)
                }
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond_expr = translate_expr(cond, dtmc);
            let cond_add = dtmc.mgr.add01_from_add(cond_expr);
            let then_add = translate_expr(then_branch, dtmc);
            let else_add = translate_expr(else_branch, dtmc);
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
    let prob = translate_expr(&update.prob, dtmc);

    let assigned_vars: HashSet<String> = update
        .assignments
        .iter()
        .filter_map(|assignment| get_assign_target(assignment).map(|name| name.to_string()))
        .collect();

    let symbolic_updates: Vec<AddNode> = update
        .assignments
        .iter()
        .map(|assignment| translate_expr(assignment, dtmc))
        .collect::<Vec<_>>();

    let mgr = &mut dtmc.mgr;
    let add_one = mgr.add_const(1.0);
    let mut assign = symbolic_updates
        .iter()
        .fold(add_one, |acc, &result| mgr.add_times(acc, result));

    for var_name in module_local_vars {
        if assigned_vars.contains(var_name) {
            continue;
        }
        let curr_nodes = dtmc.var_curr_nodes[var_name].clone();
        let next_nodes = dtmc.var_next_nodes[var_name].clone();
        for (curr, next) in curr_nodes.into_iter().zip(next_nodes.into_iter()) {
            mgr.ref_node(curr);
            mgr.ref_node(next);
            let eq = mgr.add01_equals(Add01Node(curr), Add01Node(next));
            let eq_add = mgr.add01_to_add(eq);
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
    let guard = translate_expr(&cmd.guard, dtmc);
    let updates = cmd
        .updates
        .iter()
        .map(|update| translate_update(update, module_local_vars, dtmc))
        .collect::<Vec<_>>();

    let mgr = &mut dtmc.mgr;
    let updates_sum = updates
        .iter()
        .fold(mgr.add_zero(), |acc, &update| mgr.add_plus(acc, update));
    let transition = mgr.add_times(guard, updates_sum);
    SymbolicCommand { transition }
}

/// Translate one module into identity and per-action command transitions.
fn translate_module(module: &Module, dtmc: &mut SymbolicDTMC) -> SymbolicModule {
    let module_local_vars = module
        .local_vars
        .iter()
        .map(|v| v.name.clone())
        .collect::<Vec<_>>();

    let mut ident = dtmc.mgr.add01_one();
    for var_name in module.local_vars.iter().map(|v| &v.name) {
        let curr_nodes = dtmc.var_curr_nodes[var_name].clone();
        let next_nodes = dtmc.var_next_nodes[var_name].clone();
        for (curr, next) in curr_nodes.into_iter().zip(next_nodes.into_iter()) {
            dtmc.mgr.ref_node(curr);
            dtmc.mgr.ref_node(next);
            let eq = dtmc.mgr.add01_equals(Add01Node(curr), Add01Node(next));
            ident = dtmc.mgr.add01_and(ident, eq);
        }
    }
    let ident = dtmc.mgr.add01_to_add(ident);

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
        ident,
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
    let symbolic_modules = translate_modules(dtmc);

    let mut transitions = dtmc.mgr.add_zero();
    for (act, act_modules) in &dtmc.info.modules_of_act {
        trace!("Action '{}' is part of {:?}", act, act_modules);
        let mut act_trans = dtmc.mgr.add_const(1.0);

        for module_name in dtmc.ast.modules.iter().map(|m| &m.name) {
            if act_modules.contains(module_name) {
                let mut act_mod_trans = dtmc.mgr.add_zero();
                for cmd in &symbolic_modules[module_name].commands_by_action[act] {
                    dtmc.mgr.ref_node(cmd.transition.0);
                    act_mod_trans = dtmc.mgr.add_plus(act_mod_trans, cmd.transition);
                }
                act_trans = dtmc.mgr.add_times(act_trans, act_mod_trans);
            } else {
                let ident = symbolic_modules[module_name].ident;
                dtmc.mgr.ref_node(ident.0);
                act_trans = dtmc.mgr.add_times(act_trans, ident);
            }
        }

        transitions = dtmc.mgr.add_plus(transitions, act_trans);
    }

    for module in symbolic_modules.values() {
        dtmc.mgr.deref_node(module.ident.0);
        for cmds in module.commands_by_action.values() {
            for cmd in cmds {
                dtmc.mgr.deref_node(cmd.transition.0);
            }
        }
    }

    transitions = dtmc.mgr.unif(transitions, dtmc.next_var_cube);
    dtmc.mgr.deref_node(dtmc.transitions.0);
    dtmc.transitions = transitions;
}

/// Top-level symbolic DTMC construction pipeline.
pub fn build_symbolic_dtmc(ast: DTMCAst, model_info: DTMCModelInfo) -> SymbolicDTMC {
    let mut dtmc = SymbolicDTMC::new(ast, model_info);
    allocate_dd_vars(&mut dtmc);
    translate_dtmc(&mut dtmc);
    println!("Constructed Transition ADD");
    compute_reachable_and_filter(&mut dtmc);

    dtmc.mgr
        .dump_add_dot(dtmc.transitions, "tmp.dot", &dtmc.dd_var_names)
        .unwrap();
    dtmc
}
