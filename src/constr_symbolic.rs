use crate::analyze::*;
use crate::ast::*;
use crate::ref_manager::RefManager;
use lumindd::NodeId;
use tracing::debug;

pub struct SymbolicDTMC<'a> {
    /// ManagerRef
    pub manager: RefManager,

    /// AST
    pub ast: &'a DTMCAst,

    /// Info
    pub info: &'a DTMCModelInfo,

    /// Variable name to DD node IDs, from LSB to MSB
    pub var_curr_nodes: std::collections::HashMap<String, Vec<NodeId>>,
    pub var_next_nodes: std::collections::HashMap<String, Vec<NodeId>>,

    /// ADD representing the transition relation
    pub transitions: NodeId,

    /// All primed variables BDD cube
    pub next_var_cube: NodeId,

    /// All current variables BDD cube
    pub curr_var_cube: NodeId,
}

fn allocate_dd_vars(symbolic_dtmc: &mut SymbolicDTMC) {
    for module in &symbolic_dtmc.ast.modules {
        for var_decl in &module.local_vars {
            let var_name = &var_decl.name;
            let var_type = &var_decl.var_type;
            let num_bits = match var_type {
                VarType::Bool => 1,
                VarType::BoundedInt { lo, hi } => {
                    // For simplicity, we assume lo and hi are integer literals
                    let lo_val = match **lo {
                        Expr::IntLit(val) => val,
                        _ => panic!("Expected integer literal for variable bounds"),
                    };
                    let hi_val = match **hi {
                        Expr::IntLit(val) => val,
                        _ => panic!("Expected integer literal for variable bounds"),
                    };
                    let range_size = hi_val - lo_val + 1;

                    match range_size {
                        0 => panic!("Invalid variable bounds: lo must be <= hi"),
                        1 => panic!("Variable '{}' has only one possible value", var_name), // No bits needed for a single value
                        _ => (range_size - 1).ilog2() + 1,
                    }
                }
            };

            let mgr = &mut symbolic_dtmc.manager;

            // Interleaved ordering
            let nodes: Vec<NodeId> = (0..num_bits * 2).map(|_| mgr.bdd_new_var()).collect();
            let curr_nodes: Vec<NodeId> = nodes.chunks(2).map(|c| c[0]).collect();
            let next_nodes: Vec<NodeId> = nodes.chunks(2).map(|c| c[1]).collect();

            symbolic_dtmc.curr_var_cube =
                curr_nodes
                    .iter()
                    .fold(symbolic_dtmc.curr_var_cube, |cube, &node| {
                        mgr.ref_node(node);
                        mgr.bdd_and(cube, node)
                    });
            symbolic_dtmc.next_var_cube =
                next_nodes
                    .iter()
                    .fold(symbolic_dtmc.next_var_cube, |cube, &node| {
                        mgr.ref_node(node);
                        mgr.bdd_and(cube, node)
                    });

            symbolic_dtmc
                .var_curr_nodes
                .insert(var_name.clone(), curr_nodes);
            symbolic_dtmc
                .var_next_nodes
                .insert(var_name.clone(), next_nodes);

            debug!(
                "Allocated variable '{}' with current BDD variables: {:?}",
                var_name, symbolic_dtmc.var_curr_nodes[var_name]
            );
            debug!(
                "Allocated variable '{}' with next BDD variables: {:?}",
                var_name, symbolic_dtmc.var_next_nodes[var_name]
            );
        }
    }
}

#[derive(Debug)]
struct SymbolicCommand {
    transition: NodeId,
}

#[derive(Debug)]
struct SymbolicModule {
    ident: NodeId,
    commands_by_action: std::collections::HashMap<String, Vec<SymbolicCommand>>,
}

fn get_variable_encoding(symbolic_dtmc: &mut SymbolicDTMC, var_name: &str, primed: bool) -> NodeId {
    let (lo, _) = symbolic_dtmc
        .info
        .var_bounds
        .get(var_name)
        .expect(&format!("Variable '{}' not found in model info", var_name));

    let mgr = &mut symbolic_dtmc.manager;
    let offset_add = mgr.add_const(*lo as f64);
    let variable_nodes = if primed {
        &symbolic_dtmc.var_next_nodes[var_name]
    } else {
        &symbolic_dtmc.var_curr_nodes[var_name]
    };
    let encoding = mgr.get_encoding(&variable_nodes);
    mgr.add_plus(encoding, offset_add)
}

fn translate_expr(expr: &Expr, symbolic_dtmc: &mut SymbolicDTMC) -> NodeId {
    match expr {
        Expr::IntLit(i) => symbolic_dtmc.manager.add_const(*i as f64),
        Expr::FloatLit(f) => symbolic_dtmc.manager.add_const(*f),
        Expr::BoolLit(b) => symbolic_dtmc.manager.add_const(if *b { 1.0 } else { 0.0 }),
        Expr::Ident(name) => get_variable_encoding(symbolic_dtmc, name, false),
        Expr::PrimedIdent(name) => get_variable_encoding(symbolic_dtmc, name, true),
        Expr::BinOp { lhs, op, rhs } => {
            let left = translate_expr(lhs, symbolic_dtmc);
            let right = translate_expr(rhs, symbolic_dtmc);
            match op {
                BinOp::Plus => symbolic_dtmc.manager.add_plus(left, right),
                BinOp::Minus => symbolic_dtmc.manager.add_minus(left, right),
                BinOp::Mul => symbolic_dtmc.manager.add_times(left, right),
                BinOp::Div => symbolic_dtmc.manager.add_divide(left, right),
                BinOp::Eq => symbolic_dtmc.manager.add_equals(left, right),
                BinOp::Neq => symbolic_dtmc.manager.add_nequals(left, right),
                BinOp::Lt => symbolic_dtmc.manager.add_less_than(left, right),
                BinOp::Leq => symbolic_dtmc.manager.add_less_or_equal(left, right),
                BinOp::Gt => symbolic_dtmc.manager.add_greater_than(left, right),
                BinOp::Geq => symbolic_dtmc.manager.add_greater_or_equal(left, right),
                BinOp::And | BinOp::Or => todo!(),
            }
        }
        _ => todo!(),
    }
}

fn translate_update(update: &ProbUpdate, symbolic_dtmc: &mut SymbolicDTMC) -> NodeId {
    let prob = translate_expr(&update.prob, symbolic_dtmc);
    let symbolic_updates = update
        .assignments
        .iter()
        .map(|assignment| translate_expr(&*assignment, symbolic_dtmc))
        .collect::<Vec<_>>();
    let mgr = &mut symbolic_dtmc.manager;
    let assign = symbolic_updates
        .iter()
        .fold(mgr.one(), |acc, &result| mgr.add_times(acc, result));
    symbolic_dtmc.manager.add_times(prob, assign)
}

fn translate_command(cmd: &Command, symbolic_dtmc: &mut SymbolicDTMC) -> SymbolicCommand {
    let guard = translate_expr(&cmd.guard, symbolic_dtmc);
    let updates = cmd
        .updates
        .iter()
        .map(|update| translate_update(update, symbolic_dtmc))
        .collect::<Vec<_>>();

    let mgr = &mut symbolic_dtmc.manager;
    let transition = updates
        .iter()
        .fold(guard, |acc, &update| mgr.add_times(acc, update));

    SymbolicCommand { transition }
}

fn translate_module(module: &Module, symbolic_dtmc: &mut SymbolicDTMC) -> SymbolicModule {
    let mgr = &mut symbolic_dtmc.manager;

    let mut ident = mgr.one();
    for var_name in module.local_vars.iter().map(|v| &v.name) {
        let curr_nodes = &symbolic_dtmc.var_curr_nodes[var_name];
        let next_nodes = &symbolic_dtmc.var_next_nodes[var_name];

        ident = curr_nodes
            .iter()
            .zip(next_nodes.iter())
            .fold(ident, |acc, (&curr, &next)| {
                let eq = mgr.bdd_equals(curr, next); // curr == next
                mgr.bdd_and(acc, eq)
            });
    }

    let mut commands_by_action: std::collections::HashMap<String, Vec<SymbolicCommand>> =
        std::collections::HashMap::new();
    for cmd in &module.commands {
        let symbolic_cmd = translate_command(cmd, symbolic_dtmc);
        assert!(
            !cmd.labels.len() == 1,
            "DTMCs should have exactly one label per command after analysis"
        );
        let action = &cmd.labels[0];
        commands_by_action
            .entry(action.clone())
            .or_insert_with(Vec::new)
            .push(symbolic_cmd);
    }

    SymbolicModule {
        ident,
        commands_by_action,
    }
}

pub fn build_symbolic_dtmc<'a>(
    ast: &'a DTMCAst,
    model_info: &'a DTMCModelInfo,
) -> SymbolicDTMC<'a> {
    let mut symbolic_info = SymbolicDTMC {
        var_curr_nodes: std::collections::HashMap::new(),
        var_next_nodes: std::collections::HashMap::new(),
        manager: RefManager::new(),
        transitions: NodeId::ZERO,
        next_var_cube: NodeId::ONE,
        curr_var_cube: NodeId::ONE,
        ast,
        info: model_info,
    };
    symbolic_info.manager.ref_node(symbolic_info.next_var_cube);
    symbolic_info.manager.ref_node(symbolic_info.curr_var_cube);
    symbolic_info.manager.ref_node(symbolic_info.transitions);

    allocate_dd_vars(&mut symbolic_info);

    let symbolic_modules: Vec<SymbolicModule> = symbolic_info
        .ast
        .modules
        .iter()
        .map(|module| translate_module(module, &mut symbolic_info))
        .collect();

    for (action, module_names) in &model_info.synchronisation_labels {
        debug!("Action '{}' is used by modules: {:?}", action, module_names);
    }

    println!("Symbolic modules: {:?}", symbolic_modules);

    symbolic_info
}
