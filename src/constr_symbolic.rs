use std::collections::{HashMap, HashSet};

#[allow(unused_imports)]
use tracing::{debug, info, trace};

use crate::analyze::DTMCModelInfo;
use crate::ast::*;
use crate::dd_manager::AddNode;
use crate::dd_manager::dd;
use crate::dd_manager::protected_local::{ProtectedAddLocal, ProtectedBddLocal};
use crate::dd_manager::protected_slot::{ProtectedAddSlot, ProtectedBddSlot};
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

                let curr_node = dd::bdd_var(mgr, curr);
                dtmc.var_node_roots.push(ProtectedBddSlot::new(curr_node));
                let next_node = dd::bdd_var(mgr, next);
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
    dtmc.curr_var_indices = curr_var_indices;
    dtmc.next_var_indices = next_var_indices;

    let curr_var_set = dd::var_set_from_indices(&dtmc.mgr, &dtmc.curr_var_indices);
    dtmc.curr_var_set.set(curr_var_set);
    let next_var_set = dd::var_set_from_indices(&dtmc.mgr, &dtmc.next_var_indices);
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
    let offset_add = ProtectedAddLocal::new(dd::add_const(mgr, *lo as f64));
    let variable_nodes = if primed {
        &dtmc.next_name_to_indices[var_name]
    } else {
        &dtmc.curr_name_to_indices[var_name]
    };
    let encoding = ProtectedAddLocal::new(dd::get_encoding(mgr, variable_nodes));
    dd::add_plus(mgr, encoding.get(), offset_add.get())
}

/// Translate an AST expression to a referenced ADD node.
///
/// This is shared by symbolic construction and symbolic property checking to
/// keep state-formula semantics consistent.
pub fn translate_expr(expr: &Expr, dtmc: &mut SymbolicDTMC) -> AddNode {
    match expr {
        Expr::IntLit(i) => dd::add_const(&dtmc.mgr, *i as f64),
        Expr::FloatLit(f) => dd::add_const(&dtmc.mgr, *f),
        Expr::BoolLit(b) => dd::add_const(&dtmc.mgr, if *b { 1.0 } else { 0.0 }),
        Expr::Ident(name) => get_variable_encoding(dtmc, name, false),
        Expr::PrimedIdent(name) => get_variable_encoding(dtmc, name, true),
        Expr::LabelRef(name) => {
            panic!(
                "Unresolved label reference should not reach symbolic translation: \"{}\"",
                name
            )
        }
        Expr::UnaryOp { op, operand } => {
            let value = ProtectedAddLocal::new(translate_expr(operand, dtmc));
            match op {
                UnOp::Not => {
                    let one = ProtectedAddLocal::new(dd::add_const(&dtmc.mgr, 1.0));
                    dd::add_minus(&mut dtmc.mgr, one.get(), value.get())
                }
                UnOp::Neg => {
                    let zero = ProtectedAddLocal::new(dd::add_const(&dtmc.mgr, 0.0));
                    dd::add_minus(&mut dtmc.mgr, zero.get(), value.get())
                }
            }
        }
        Expr::BinOp { lhs, op, rhs } => {
            let left = ProtectedAddLocal::new(translate_expr(lhs, dtmc));
            let right = ProtectedAddLocal::new(translate_expr(rhs, dtmc));
            match op {
                BinOp::Plus => dd::add_plus(&mut dtmc.mgr, left.get(), right.get()),
                BinOp::Minus => dd::add_minus(&mut dtmc.mgr, left.get(), right.get()),
                BinOp::Mul => dd::add_times(&mut dtmc.mgr, left.get(), right.get()),
                BinOp::Div => dd::add_divide(&mut dtmc.mgr, left.get(), right.get()),
                BinOp::Eq => {
                    let bdd = ProtectedBddLocal::new(dd::add_equals(
                        &mut dtmc.mgr,
                        left.get(),
                        right.get(),
                    ));
                    dd::bdd_to_add(&mut dtmc.mgr, bdd.get())
                }
                BinOp::Neq => {
                    let bdd = ProtectedBddLocal::new(dd::add_nequals(
                        &mut dtmc.mgr,
                        left.get(),
                        right.get(),
                    ));
                    dd::bdd_to_add(&mut dtmc.mgr, bdd.get())
                }
                BinOp::Lt => {
                    let bdd = ProtectedBddLocal::new(dd::add_less_than(
                        &mut dtmc.mgr,
                        left.get(),
                        right.get(),
                    ));
                    dd::bdd_to_add(&mut dtmc.mgr, bdd.get())
                }
                BinOp::Leq => {
                    let bdd = ProtectedBddLocal::new(dd::add_less_or_equal(
                        &mut dtmc.mgr,
                        left.get(),
                        right.get(),
                    ));
                    dd::bdd_to_add(&mut dtmc.mgr, bdd.get())
                }
                BinOp::Gt => {
                    let bdd = ProtectedBddLocal::new(dd::add_greater_than(
                        &mut dtmc.mgr,
                        left.get(),
                        right.get(),
                    ));
                    dd::bdd_to_add(&mut dtmc.mgr, bdd.get())
                }
                BinOp::Geq => {
                    let bdd = ProtectedBddLocal::new(dd::add_greater_or_equal(
                        &mut dtmc.mgr,
                        left.get(),
                        right.get(),
                    ));
                    dd::bdd_to_add(&mut dtmc.mgr, bdd.get())
                }
                BinOp::And => dd::add_times(&mut dtmc.mgr, left.get(), right.get()),
                BinOp::Or => {
                    let add01_left =
                        ProtectedBddLocal::new(dd::add_to_bdd(&mut dtmc.mgr, left.get()));
                    let add01_right =
                        ProtectedBddLocal::new(dd::add_to_bdd(&mut dtmc.mgr, right.get()));
                    let bdd_or = ProtectedBddLocal::new(dd::bdd_or(
                        &dtmc.mgr,
                        add01_left.get(),
                        add01_right.get(),
                    ));
                    dd::bdd_to_add(&mut dtmc.mgr, bdd_or.get())
                }
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond_expr = ProtectedAddLocal::new(translate_expr(cond, dtmc));
            let cond_add = ProtectedBddLocal::new(dd::add_to_bdd(&mut dtmc.mgr, cond_expr.get()));
            let then_add = ProtectedAddLocal::new(translate_expr(then_branch, dtmc));
            let else_add = ProtectedAddLocal::new(translate_expr(else_branch, dtmc));
            dd::add_ite(
                &mut dtmc.mgr,
                cond_add.get(),
                then_add.get(),
                else_add.get(),
            )
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
    let prob = ProtectedAddLocal::new(translate_expr(&update.prob, dtmc));

    let assigned_vars: HashSet<String> = update
        .assignments
        .iter()
        .filter_map(|assignment| get_assign_target(assignment).map(|name| name.to_string()))
        .collect();

    let symbolic_updates: Vec<ProtectedAddLocal> = update
        .assignments
        .iter()
        .map(|assignment| ProtectedAddLocal::new(translate_expr(assignment, dtmc)))
        .collect::<Vec<_>>();

    let mgr = &mut dtmc.mgr;
    let mut assign = ProtectedAddLocal::new(dd::add_const(mgr, 1.0));
    for symbolic_update in &symbolic_updates {
        assign.set(dd::add_times(mgr, assign.get(), symbolic_update.get()));
    }

    for var_name in module_local_vars {
        if assigned_vars.contains(var_name) {
            continue;
        }
        let curr_nodes = dtmc.curr_name_to_indices[var_name].clone();
        let next_nodes = dtmc.next_name_to_indices[var_name].clone();
        for (curr, next) in curr_nodes.into_iter().zip(next_nodes.into_iter()) {
            let curr_var = ProtectedBddLocal::new(dd::bdd_var(mgr, curr));
            let next_var = ProtectedBddLocal::new(dd::bdd_var(mgr, next));
            let eq = ProtectedBddLocal::new(dd::bdd_equals(mgr, curr_var.get(), next_var.get()));
            let eq_add = ProtectedAddLocal::new(dd::bdd_to_add(mgr, eq.get()));
            assign.set(dd::add_times(mgr, assign.get(), eq_add.get()));
        }
    }

    dd::add_times(&mut dtmc.mgr, prob.get(), assign.get())
}

/// Translate one command: `guard * (sum updates)`.
fn translate_command(
    cmd: &Command,
    module_local_vars: &[String],
    dtmc: &mut SymbolicDTMC,
) -> SymbolicCommand {
    let cmd_guard = ProtectedAddLocal::new(translate_expr(&cmd.guard, dtmc));
    let updates = cmd
        .updates
        .iter()
        .map(|update| ProtectedAddLocal::new(translate_update(update, module_local_vars, dtmc)))
        .collect::<Vec<_>>();

    let mgr = &mut dtmc.mgr;
    let mut updates_sum = ProtectedAddLocal::new(dd::add_zero(mgr));
    for update in &updates {
        updates_sum.set(dd::add_plus(mgr, updates_sum.get(), update.get()));
    }
    let transition = dd::add_times(mgr, cmd_guard.get(), updates_sum.get());
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

    let mut ident = ProtectedBddLocal::new(dd::bdd_one(&mut dtmc.mgr));
    for var_name in module.local_vars.iter().map(|v| &v.name) {
        let curr_nodes = dtmc.curr_name_to_indices[var_name].clone();
        let next_nodes = dtmc.next_name_to_indices[var_name].clone();
        for (curr, next) in curr_nodes.into_iter().zip(next_nodes.into_iter()) {
            let curr_var = ProtectedBddLocal::new(dd::bdd_var(&dtmc.mgr, curr));
            let next_var = ProtectedBddLocal::new(dd::bdd_var(&dtmc.mgr, next));
            let eq =
                ProtectedBddLocal::new(dd::bdd_equals(&dtmc.mgr, curr_var.get(), next_var.get()));
            ident.set(dd::bdd_and(&dtmc.mgr, ident.get(), eq.get()));
        }
    }
    let ident = dd::bdd_to_add(&mut dtmc.mgr, ident.get());

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

    let mut transitions = ProtectedAddLocal::new(dd::add_zero(&dtmc.mgr));
    for (act, act_modules) in &dtmc.info.modules_of_act {
        trace!("Action '{}' is part of {:?}", act, act_modules);
        let mut act_trans = ProtectedAddLocal::new(dd::add_const(&dtmc.mgr, 1.0));

        for module_name in dtmc.ast.modules.iter().map(|m| &m.name) {
            if act_modules.contains(module_name) {
                let mut act_mod_trans = ProtectedAddLocal::new(dd::add_zero(&dtmc.mgr));
                for cmd in &symbolic_modules[module_name].commands_by_action[act] {
                    act_mod_trans.set(dd::add_plus(
                        &mut dtmc.mgr,
                        act_mod_trans.get(),
                        cmd.transition.get(),
                    ));
                }
                act_trans.set(dd::add_times(
                    &mut dtmc.mgr,
                    act_trans.get(),
                    act_mod_trans.get(),
                ));
            } else {
                act_trans.set(dd::add_times(
                    &mut dtmc.mgr,
                    act_trans.get(),
                    symbolic_modules[module_name].ident.get(),
                ));
            }
        }

        transitions.set(dd::add_plus(
            &mut dtmc.mgr,
            transitions.get(),
            act_trans.get(),
        ));
    }

    transitions.set(dd::unif(
        &mut dtmc.mgr,
        transitions.get(),
        dtmc.next_var_cube.get(),
    ));
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
