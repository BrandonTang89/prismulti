use std::collections::{HashMap, HashSet};

#[allow(unused_imports)]
use tracing::{debug, info, trace};

use crate::analyze::DTMCModelInfo;
use crate::ast::*;
use crate::dd_manager::AddNode;
use crate::dd_manager::dd;
use crate::dd_manager::protected_slot::ProtectedAddSlot;
use crate::protected_add;
use crate::protected_bdd;
use crate::reachability::compute_reachable_and_filter;
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
                var_name, dtmc.curr_name_to_indices[var_name]
            );
            trace!(
                "Allocated var '{}' with next BDD vars: {:?}",
                var_name, dtmc.next_name_to_indices[var_name]
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

    let curr_var_set = dd::var_set_from_indices(&curr_var_indices);
    dtmc.curr_var_set.set(curr_var_set);
    let next_var_set = dd::var_set_from_indices(&next_var_indices);
    dtmc.next_var_set.set(next_var_set);

    dtmc.curr_to_next_map.set(dd::build_swap_map(
        &dtmc.mgr,
        &curr_var_indices,
        &next_var_indices,
    ));

    dtmc.curr_var_indices = curr_var_indices;
    dtmc.next_var_indices = next_var_indices;
}

/// Return ADD encoding of variable value (`curr` or `next`) with lower-bound offset.
fn get_variable_encoding(dtmc: &mut SymbolicDTMC, var_name: &str, primed: bool) -> AddNode {
    let (lo, _) = dtmc
        .info
        .var_bounds
        .get(var_name)
        .unwrap_or_else(|| panic!("Variable '{}' not found in model info", var_name));

    let mgr = &mut dtmc.mgr;
    protected_add!(offset_add, dd::add_const(*lo as f64));
    let variable_nodes = if primed {
        &dtmc.next_name_to_indices[var_name]
    } else {
        &dtmc.curr_name_to_indices[var_name]
    };
    protected_add!(encoding, dd::get_encoding(mgr, variable_nodes));
    dd::add_plus(encoding.get(), offset_add.get())
}

/// Translate an AST expression to a referenced ADD node.
///
/// This is shared by symbolic construction and symbolic property checking to
/// keep state-formula semantics consistent.
pub fn translate_expr(expr: &Expr, dtmc: &mut SymbolicDTMC) -> AddNode {
    match expr {
        Expr::IntLit(i) => dd::add_const(*i as f64),
        Expr::FloatLit(f) => dd::add_const(*f),
        Expr::BoolLit(b) => dd::add_const(if *b { 1.0 } else { 0.0 }),
        Expr::Ident(name) => get_variable_encoding(dtmc, name, false),
        Expr::PrimedIdent(name) => get_variable_encoding(dtmc, name, true),
        Expr::LabelRef(name) => {
            panic!(
                "Unresolved label reference should not reach symbolic translation: \"{}\"",
                name
            )
        }
        Expr::UnaryOp { op, operand } => {
            protected_add!(value, translate_expr(operand, dtmc));
            match op {
                UnOp::Not => {
                    protected_add!(one, dd::add_const(1.0));
                    dd::add_minus(one.get(), value.get())
                }
                UnOp::Neg => {
                    protected_add!(zero, dd::add_const(0.0));
                    dd::add_minus(zero.get(), value.get())
                }
            }
        }
        Expr::BinOp { lhs, op, rhs } => {
            protected_add!(left, translate_expr(lhs, dtmc));
            protected_add!(right, translate_expr(rhs, dtmc));
            match op {
                BinOp::Plus => dd::add_plus(left.get(), right.get()),
                BinOp::Minus => dd::add_minus(left.get(), right.get()),
                BinOp::Mul => dd::add_times(left.get(), right.get()),
                BinOp::Div => dd::add_divide(left.get(), right.get()),
                BinOp::Eq => {
                    protected_bdd!(bdd, dd::add_equals(left.get(), right.get()));
                    dd::bdd_to_add(bdd.get())
                }
                BinOp::Neq => {
                    protected_bdd!(bdd, dd::add_nequals(left.get(), right.get()));
                    dd::bdd_to_add(bdd.get())
                }
                BinOp::Lt => {
                    protected_bdd!(bdd, dd::add_less_than(left.get(), right.get()));
                    dd::bdd_to_add(bdd.get())
                }
                BinOp::Leq => {
                    protected_bdd!(bdd, dd::add_less_or_equal(left.get(), right.get()));
                    dd::bdd_to_add(bdd.get())
                }
                BinOp::Gt => {
                    protected_bdd!(bdd, dd::add_greater_than(left.get(), right.get()));
                    dd::bdd_to_add(bdd.get())
                }
                BinOp::Geq => {
                    protected_bdd!(bdd, dd::add_greater_or_equal(left.get(), right.get()));
                    dd::bdd_to_add(bdd.get())
                }
                BinOp::And => dd::add_times(left.get(), right.get()),
                BinOp::Or => {
                    protected_bdd!(add01_left, dd::add_to_bdd(left.get()));
                    protected_bdd!(add01_right, dd::add_to_bdd(right.get()));
                    protected_bdd!(bdd_or, dd::bdd_or(add01_left.get(), add01_right.get()));
                    dd::bdd_to_add(bdd_or.get())
                }
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            protected_add!(cond_expr, translate_expr(cond, dtmc));
            protected_bdd!(cond_add, dd::add_to_bdd(cond_expr.get()));
            protected_add!(then_add, translate_expr(then_branch, dtmc));
            protected_add!(else_add, translate_expr(else_branch, dtmc));
            dd::add_ite(cond_add.get(), then_add.get(), else_add.get())
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
    protected_add!(prob, translate_expr(&update.prob, dtmc));

    let assigned_vars: HashSet<String> = update
        .assignments
        .iter()
        .filter_map(|assignment| get_assign_target(assignment).map(|name| name.to_string()))
        .collect();

    protected_add!(assign, dd::add_const(1.0));
    for assignment in &update.assignments {
        protected_add!(symbolic_update, translate_expr(assignment, dtmc));
        assign.set(dd::add_times(assign.get(), symbolic_update.get()));
    }

    for var_name in module_local_vars {
        if assigned_vars.contains(var_name) {
            continue;
        }
        let curr_nodes = dtmc.curr_name_to_indices[var_name].clone();
        let next_nodes = dtmc.next_name_to_indices[var_name].clone();
        for (curr, next) in curr_nodes.into_iter().zip(next_nodes) {
            protected_bdd!(curr_var, dd::bdd_var(&dtmc.mgr, curr));
            protected_bdd!(next_var, dd::bdd_var(&dtmc.mgr, next));
            protected_bdd!(eq, dd::bdd_equals(curr_var.get(), next_var.get()));
            protected_add!(eq_add, dd::bdd_to_add(eq.get()));
            assign.set(dd::add_times(assign.get(), eq_add.get()));
        }
    }

    dd::add_times(prob.get(), assign.get())
}

/// Translate one command: `guard * (sum updates)`.
fn translate_command(
    cmd: &Command,
    module_local_vars: &[String],
    dtmc: &mut SymbolicDTMC,
) -> SymbolicCommand {
    protected_add!(cmd_guard, translate_expr(&cmd.guard, dtmc));

    protected_add!(updates_sum, dd::add_zero());
    for update in &cmd.updates {
        protected_add!(
            symbolic_update,
            translate_update(update, module_local_vars, dtmc)
        );
        updates_sum.set(dd::add_plus(updates_sum.get(), symbolic_update.get()));
    }
    let transition = dd::add_times(cmd_guard.get(), updates_sum.get());
    SymbolicCommand {
        transition: ProtectedAddSlot::new(transition),
    }
}

/// Translate one module into identity and per-action command transitions.
fn translate_module(module: &Module, dtmc: &mut SymbolicDTMC) -> SymbolicModule {
    let module_local_vars = module
        .local_vars
        .iter()
        .map(|v| v.name.clone())
        .collect::<Vec<_>>();

    protected_bdd!(ident, dd::bdd_one());
    for var_name in module.local_vars.iter().map(|v| &v.name) {
        let curr_nodes = dtmc.curr_name_to_indices[var_name].clone();
        let next_nodes = dtmc.next_name_to_indices[var_name].clone();
        for (curr, next) in curr_nodes.into_iter().zip(next_nodes) {
            protected_bdd!(curr_var, dd::bdd_var(&dtmc.mgr, curr));
            protected_bdd!(next_var, dd::bdd_var(&dtmc.mgr, next));
            protected_bdd!(eq, dd::bdd_equals(curr_var.get(), next_var.get()));
            ident.set(dd::bdd_and(ident.get(), eq.get()));
        }
    }
    let ident = dd::bdd_to_add(ident.get());

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
    let symbolic_modules = translate_modules(dtmc);

    protected_add!(transitions, dd::add_zero());
    for (act, act_modules) in &dtmc.info.modules_of_act {
        trace!("Action '{}' is part of {:?}", act, act_modules);
        protected_add!(act_trans, dd::add_const(1.0));

        for module_name in dtmc.ast.modules.iter().map(|m| &m.name) {
            if act_modules.contains(module_name) {
                protected_add!(act_mod_trans, dd::add_zero());
                for cmd in &symbolic_modules[module_name].commands_by_action[act] {
                    act_mod_trans.set(dd::add_plus(act_mod_trans.get(), cmd.transition.get()));
                }
                act_trans.set(dd::add_times(act_trans.get(), act_mod_trans.get()));
            } else {
                act_trans.set(dd::add_times(
                    act_trans.get(),
                    symbolic_modules[module_name].ident.get(),
                ));
            }
        }

        transitions.set(dd::add_plus(transitions.get(), act_trans.get()));
    }

    transitions.set(dd::unif(transitions.get(), dtmc.next_var_set.get()));
    dtmc.transitions.replace(transitions.get());
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
