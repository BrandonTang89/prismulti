//! Semantic analysis and normalization for models.
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use anyhow::{Result, anyhow, bail};

#[derive(Clone, Debug)]
pub struct BasicModelInfo {
    pub module_names: Vec<String>,

    /// action label -> Vec(modules with commands with this label)
    pub modules_of_act: HashMap<String, Vec<String>>,

    /// LocalVarName -> ModuleName
    pub module_of_var: HashMap<String, String>,

    /// VariableName -> (lo, hi)
    pub var_bounds: HashMap<String, (i32, i32)>,
}

pub type DTMCModelInfo = BasicModelInfo;
pub type MDPModelInfo = BasicModelInfo;

pub fn analyse_dtmc(
    model: &mut DTMCAst,
    const_overrides: &HashMap<String, String>,
) -> Result<DTMCModelInfo> {
    let info = analyse_basic_model(&mut model.basic, const_overrides)?;

    let (symbol_types, constant_values) = get_symbol_table(&model.basic, const_overrides)?;
    let labels_by_name = analyze_labels(&mut model.labels, &symbol_types, &constant_values)?;

    analyze_properties(
        &mut model.properties,
        &symbol_types,
        &constant_values,
        &labels_by_name,
    )?;

    Ok(info)
}

pub fn analyse_mdp(
    model: &mut MDPAst,
    const_overrides: &HashMap<String, String>,
) -> Result<MDPModelInfo> {
    let info = analyse_basic_model(&mut model.basic, const_overrides)?;

    let (symbol_types, constant_values) = get_symbol_table(&model.basic, const_overrides)?;
    let labels_by_name = analyze_labels(&mut model.labels, &symbol_types, &constant_values)?;

    analyze_properties(
        &mut model.properties,
        &symbol_types,
        &constant_values,
        &labels_by_name,
    )?;

    Ok(info)
}

fn get_symbol_table(
    model: &BasicAst,
    const_overrides: &HashMap<String, String>,
) -> Result<(HashMap<String, TypeKind>, HashMap<String, Expr>)> {
    let mut symbol_types: HashMap<String, TypeKind> = HashMap::new();
    for (name, decl) in &model.constants {
        symbol_types.insert(name.clone(), const_type_to_kind(&decl.const_type));
    }
    for module in &model.modules {
        for var_decl in &module.local_vars {
            let kind = var_type_to_kind(&var_decl.var_type);
            symbol_types.insert(var_decl.name.clone(), kind);
        }
    }

    let mut model_copy = model.clone();
    let constant_values =
        apply_and_resolve_constants(&mut model_copy, &symbol_types, const_overrides)?;

    Ok((symbol_types, constant_values))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeKind {
    Bool,
    Int,
    Float,
}

impl TypeKind {
    fn as_str(self) -> &'static str {
        match self {
            TypeKind::Bool => "bool",
            TypeKind::Int => "int",
            TypeKind::Float => "float",
        }
    }
}

fn const_type_to_kind(const_type: &ConstType) -> TypeKind {
    match const_type {
        ConstType::Bool => TypeKind::Bool,
        ConstType::Int => TypeKind::Int,
        ConstType::Float => TypeKind::Float,
    }
}

fn var_type_to_kind(var_type: &VarType) -> TypeKind {
    match var_type {
        VarType::Bool => TypeKind::Bool,
        VarType::BoundedInt { .. } => TypeKind::Int,
    }
}

fn const_cli_value_expr(value: &str, expected: TypeKind, name: &str) -> Result<Box<Expr>> {
    match expected {
        TypeKind::Bool => match value {
            "true" => Ok(Box::new(Expr::BoolLit(true))),
            "false" => Ok(Box::new(Expr::BoolLit(false))),
            _ => bail!(
                "Invalid bool constant override for '{}': '{}'. Expected 'true' or 'false'.",
                name,
                value
            ),
        },
        TypeKind::Int => {
            let parsed = value.parse::<i32>().map_err(|_| {
                anyhow!("Invalid int constant override for '{}': '{}'.", name, value)
            })?;
            Ok(Box::new(Expr::IntLit(parsed)))
        }
        TypeKind::Float => {
            let parsed = value.parse::<f64>().map_err(|_| {
                anyhow!(
                    "Invalid float constant override for '{}': '{}'.",
                    name,
                    value
                )
            })?;
            Ok(Box::new(Expr::FloatLit(parsed)))
        }
    }
}

fn infer_expr_type(expr: &Expr, symbol_types: &HashMap<String, TypeKind>) -> Result<TypeKind> {
    let err = |msg: String| anyhow!(msg);
    match expr {
        Expr::BoolLit(_) => Ok(TypeKind::Bool),
        Expr::IntLit(_) => Ok(TypeKind::Int),
        Expr::FloatLit(_) => Ok(TypeKind::Float),
        Expr::LabelRef(name) => Err(err(format!("Unresolved label reference '\"{}\"'.", name))),
        Expr::Ident(name) | Expr::PrimedIdent(name) => symbol_types
            .get(name)
            .copied()
            .ok_or_else(|| err(format!("Unknown identifier '{}'.", name))),
        Expr::UnaryOp { op, operand } => {
            let t = infer_expr_type(operand, symbol_types)?;
            match op {
                UnOp::Not => {
                    if t == TypeKind::Bool {
                        Ok(TypeKind::Bool)
                    } else {
                        Err(err(format!(
                            "Type error: operator '!' expects bool but found {}.",
                            t.as_str()
                        )))
                    }
                }
                UnOp::Neg => {
                    if t == TypeKind::Int || t == TypeKind::Float {
                        Ok(t)
                    } else {
                        Err(err(format!(
                            "Type error: unary '-' expects int/float but found {}.",
                            t.as_str()
                        )))
                    }
                }
            }
        }
        Expr::BinOp { lhs, op, rhs } => {
            let lt = infer_expr_type(lhs, symbol_types)?;
            let rt = infer_expr_type(rhs, symbol_types)?;
            match op {
                BinOp::And | BinOp::Or => {
                    if lt == TypeKind::Bool && rt == TypeKind::Bool {
                        Ok(TypeKind::Bool)
                    } else {
                        Err(err(format!(
                            "Type error: logical operator expects bool/bool but found {}/{}.",
                            lt.as_str(),
                            rt.as_str()
                        )))
                    }
                }
                BinOp::Plus | BinOp::Minus | BinOp::Mul | BinOp::Div => {
                    let numeric = |t: TypeKind| t == TypeKind::Int || t == TypeKind::Float;
                    if !numeric(lt) || !numeric(rt) {
                        return Err(err(format!(
                            "Type error: arithmetic operator expects numeric operands but found {}/{}.",
                            lt.as_str(),
                            rt.as_str()
                        )));
                    }
                    let produces_float =
                        matches!(op, BinOp::Div) || lt == TypeKind::Float || rt == TypeKind::Float;
                    if produces_float {
                        Ok(TypeKind::Float)
                    } else {
                        Ok(TypeKind::Int)
                    }
                }
                BinOp::Lt | BinOp::Leq | BinOp::Gt | BinOp::Geq => {
                    let numeric = |t: TypeKind| t == TypeKind::Int || t == TypeKind::Float;
                    if numeric(lt) && numeric(rt) {
                        Ok(TypeKind::Bool)
                    } else {
                        Err(err(format!(
                            "Type error: comparison expects numeric operands but found {}/{}.",
                            lt.as_str(),
                            rt.as_str()
                        )))
                    }
                }
                BinOp::Eq | BinOp::Neq => {
                    if lt == rt
                        || ((lt == TypeKind::Int || lt == TypeKind::Float)
                            && (rt == TypeKind::Int || rt == TypeKind::Float))
                    {
                        Ok(TypeKind::Bool)
                    } else {
                        Err(err(format!(
                            "Type error: equality compares incompatible types {}/{}.",
                            lt.as_str(),
                            rt.as_str()
                        )))
                    }
                }
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let ct = infer_expr_type(cond, symbol_types)?;
            if ct != TypeKind::Bool {
                return Err(err(format!(
                    "Type error: ternary condition must be bool, found {}.",
                    ct.as_str()
                )));
            }
            let tt = infer_expr_type(then_branch, symbol_types)?;
            let et = infer_expr_type(else_branch, symbol_types)?;
            if tt == et {
                Ok(tt)
            } else if (tt == TypeKind::Int || tt == TypeKind::Float)
                && (et == TypeKind::Int || et == TypeKind::Float)
            {
                Ok(TypeKind::Float)
            } else {
                Err(err(format!(
                    "Type error: ternary branches have incompatible types {}/{}.",
                    tt.as_str(),
                    et.as_str()
                )))
            }
        }
    }
}

fn ensure_type_ok(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok { Ok(()) } else { bail!(message.into()) }
}

fn expr_to_f64(expr: &Expr) -> f64 {
    match expr {
        Expr::IntLit(v) => *v as f64,
        Expr::FloatLit(v) => *v,
        Expr::BoolLit(v) => {
            if *v {
                1.0
            } else {
                0.0
            }
        }
        _ => panic!("Expected folded literal expression"),
    }
}

fn fold_expr(expr: &Expr, constant_values: &HashMap<String, Expr>) -> Expr {
    match expr {
        Expr::BoolLit(v) => Expr::BoolLit(*v),
        Expr::IntLit(v) => Expr::IntLit(*v),
        Expr::FloatLit(v) => Expr::FloatLit(*v),
        Expr::LabelRef(name) => Expr::LabelRef(name.clone()),
        Expr::Ident(name) => constant_values
            .get(name)
            .cloned()
            .unwrap_or_else(|| Expr::Ident(name.clone())),
        Expr::PrimedIdent(name) => Expr::PrimedIdent(name.clone()),
        Expr::UnaryOp { op, operand } => {
            let operand = fold_expr(operand, constant_values);
            match (op, operand) {
                (UnOp::Not, Expr::BoolLit(v)) => Expr::BoolLit(!v),
                (UnOp::Neg, Expr::IntLit(v)) => Expr::IntLit(-v),
                (UnOp::Neg, Expr::FloatLit(v)) => Expr::FloatLit(-v),
                (UnOp::Not, other) => Expr::UnaryOp {
                    op: UnOp::Not,
                    operand: Box::new(other),
                },
                (UnOp::Neg, other) => Expr::UnaryOp {
                    op: UnOp::Neg,
                    operand: Box::new(other),
                },
            }
        }
        Expr::BinOp { lhs, op, rhs } => {
            let lhs = fold_expr(lhs, constant_values);
            let rhs = fold_expr(rhs, constant_values);
            match (&lhs, op, &rhs) {
                (Expr::IntLit(a), BinOp::Plus, Expr::IntLit(b)) => Expr::IntLit(a + b),
                (Expr::IntLit(a), BinOp::Minus, Expr::IntLit(b)) => Expr::IntLit(a - b),
                (Expr::IntLit(a), BinOp::Mul, Expr::IntLit(b)) => Expr::IntLit(a * b),
                (Expr::IntLit(a), BinOp::Div, Expr::IntLit(b)) => {
                    Expr::FloatLit(*a as f64 / *b as f64)
                }
                (Expr::BoolLit(a), BinOp::And, Expr::BoolLit(b)) => Expr::BoolLit(*a && *b),
                (Expr::BoolLit(a), BinOp::Or, Expr::BoolLit(b)) => Expr::BoolLit(*a || *b),
                (Expr::BoolLit(a), BinOp::Eq, Expr::BoolLit(b)) => Expr::BoolLit(a == b),
                (Expr::BoolLit(a), BinOp::Neq, Expr::BoolLit(b)) => Expr::BoolLit(a != b),
                (Expr::IntLit(_) | Expr::FloatLit(_), _, Expr::IntLit(_) | Expr::FloatLit(_)) => {
                    let a = expr_to_f64(&lhs);
                    let b = expr_to_f64(&rhs);
                    match op {
                        BinOp::Plus => Expr::FloatLit(a + b),
                        BinOp::Minus => Expr::FloatLit(a - b),
                        BinOp::Mul => Expr::FloatLit(a * b),
                        BinOp::Div => Expr::FloatLit(a / b),
                        BinOp::Lt => Expr::BoolLit(a < b),
                        BinOp::Leq => Expr::BoolLit(a <= b),
                        BinOp::Gt => Expr::BoolLit(a > b),
                        BinOp::Geq => Expr::BoolLit(a >= b),
                        BinOp::Eq => Expr::BoolLit((a - b).abs() < 1e-12),
                        BinOp::Neq => Expr::BoolLit((a - b).abs() >= 1e-12),
                        _ => Expr::BinOp {
                            lhs: Box::new(lhs),
                            op: op.clone(),
                            rhs: Box::new(rhs),
                        },
                    }
                }
                _ => Expr::BinOp {
                    lhs: Box::new(lhs),
                    op: op.clone(),
                    rhs: Box::new(rhs),
                },
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond = fold_expr(cond, constant_values);
            let then_branch = fold_expr(then_branch, constant_values);
            let else_branch = fold_expr(else_branch, constant_values);
            match cond {
                Expr::BoolLit(true) => then_branch,
                Expr::BoolLit(false) => else_branch,
                c => Expr::Ternary {
                    cond: Box::new(c),
                    then_branch: Box::new(then_branch),
                    else_branch: Box::new(else_branch),
                },
            }
        }
    }
}

fn fold_box_expr(expr: &mut Box<Expr>, constant_values: &HashMap<String, Expr>) {
    **expr = fold_expr(expr.as_ref(), constant_values);
}

fn fold_path_formula(path: &mut PathFormula, constant_values: &HashMap<String, Expr>) {
    match path {
        PathFormula::Next(phi) => fold_box_expr(phi, constant_values),
        PathFormula::Until { lhs, rhs, bound } | PathFormula::Release { lhs, rhs, bound } => {
            fold_box_expr(lhs, constant_values);
            fold_box_expr(rhs, constant_values);
            if let Some(k) = bound {
                fold_box_expr(k, constant_values);
            }
        }
    }
}

fn expand_label_refs_in_expr(
    expr: &mut Expr,
    labels_by_name: &HashMap<String, Box<Expr>>,
    resolving: &mut Vec<String>,
) -> Result<()> {
    match expr {
        Expr::LabelRef(name) => {
            if resolving.iter().any(|n| n == name) {
                let mut cycle = resolving.clone();
                cycle.push(name.clone());
                bail!("Cyclic label definition detected: {}", cycle.join(" -> "));
            }

            let label_expr = labels_by_name
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow!("Unknown label reference '\"{}\"'.", name))?;
            resolving.push(name.clone());
            let mut expanded = *label_expr;
            expand_label_refs_in_expr(&mut expanded, labels_by_name, resolving)?;
            resolving.pop();
            *expr = expanded;
            Ok(())
        }
        Expr::UnaryOp { operand, .. } => {
            expand_label_refs_in_expr(operand.as_mut(), labels_by_name, resolving)
        }
        Expr::BinOp { lhs, rhs, .. } => {
            expand_label_refs_in_expr(lhs.as_mut(), labels_by_name, resolving)?;
            expand_label_refs_in_expr(rhs.as_mut(), labels_by_name, resolving)
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expand_label_refs_in_expr(cond.as_mut(), labels_by_name, resolving)?;
            expand_label_refs_in_expr(then_branch.as_mut(), labels_by_name, resolving)?;
            expand_label_refs_in_expr(else_branch.as_mut(), labels_by_name, resolving)
        }
        Expr::BoolLit(_)
        | Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::Ident(_)
        | Expr::PrimedIdent(_) => Ok(()),
    }
}

fn expand_label_refs_in_box_expr(
    expr: &mut Box<Expr>,
    labels_by_name: &HashMap<String, Box<Expr>>,
) -> Result<()> {
    expand_label_refs_in_expr(expr.as_mut(), labels_by_name, &mut Vec::new())
}

fn expand_label_refs_in_path_formula(
    path: &mut PathFormula,
    labels_by_name: &HashMap<String, Box<Expr>>,
) -> Result<()> {
    match path {
        PathFormula::Next(phi) => expand_label_refs_in_box_expr(phi, labels_by_name),
        PathFormula::Until { lhs, rhs, bound } | PathFormula::Release { lhs, rhs, bound } => {
            expand_label_refs_in_box_expr(lhs, labels_by_name)?;
            expand_label_refs_in_box_expr(rhs, labels_by_name)?;
            if let Some(k) = bound {
                expand_label_refs_in_box_expr(k, labels_by_name)?;
            }
            Ok(())
        }
    }
}

fn ensure_no_primed_idents(expr: &Expr, where_msg: &str) -> Result<()> {
    match expr {
        Expr::PrimedIdent(name) => bail!(
            "{}: primed identifier '{}' is not allowed in property state formulas.",
            where_msg,
            name
        ),
        Expr::UnaryOp { operand, .. } => ensure_no_primed_idents(operand, where_msg),
        Expr::BinOp { lhs, rhs, .. } => {
            ensure_no_primed_idents(lhs, where_msg)?;
            ensure_no_primed_idents(rhs, where_msg)
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            ensure_no_primed_idents(cond, where_msg)?;
            ensure_no_primed_idents(then_branch, where_msg)?;
            ensure_no_primed_idents(else_branch, where_msg)
        }
        Expr::BoolLit(_)
        | Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::Ident(_)
        | Expr::LabelRef(_) => Ok(()),
    }
}

/// Returns constant names that still have no value expression after applying
/// CLI overrides.
fn missing_constant_values(constants: &[(String, ConstDecl)]) -> Vec<String> {
    constants
        .iter()
        .filter_map(|(name, decl)| decl.value.is_none().then_some(name.clone()))
        .collect()
}

/// Converts a folded constant expression into a resolved literal if it matches
/// the declared constant type (with int-to-float promotion for float constants).
fn fold_resolved_const_value(declared_type: &ConstType, folded: Expr) -> Option<Expr> {
    match (declared_type, folded) {
        (ConstType::Bool, Expr::BoolLit(v)) => Some(Expr::BoolLit(v)),
        (ConstType::Int, Expr::IntLit(v)) => Some(Expr::IntLit(v)),
        (ConstType::Float, Expr::FloatLit(v)) => Some(Expr::FloatLit(v)),
        (ConstType::Float, Expr::IntLit(v)) => Some(Expr::FloatLit(v as f64)),
        _ => None,
    }
}

fn type_check_constant_declarations(
    constants: &[(String, ConstDecl)],
    symbol_types: &HashMap<String, TypeKind>,
) -> Result<()> {
    for (name, decl) in constants {
        let value_expr = decl
            .value
            .as_ref()
            .expect("constant missing value should have been validated before type checking");
        let inferred = infer_expr_type(value_expr, symbol_types)
            .map_err(|e| anyhow!("In constant '{}': {}", name, e))?;
        let declared = const_type_to_kind(&decl.const_type);
        ensure_type_ok(
            inferred == declared || (declared == TypeKind::Float && inferred == TypeKind::Int),
            format!(
                "Type error in constant '{}': declared {} but expression has type {}",
                name,
                declared.as_str(),
                inferred.as_str()
            ),
        )?;
    }
    Ok(())
}

/// Applies CLI constant overrides and resolves each constant declaration to a literal value.
///
/// The resolution process:
/// - first applies `--const` overrides to matching declarations,
/// - requires every constant to have an expression after overrides,
/// - type-checks each declaration against its declared constant type exactly once,
/// - folds expressions using already-resolved constants until all values become literals.
fn apply_and_resolve_constants_for_decls(
    constants: &mut [(String, ConstDecl)],
    symbol_types: &HashMap<String, TypeKind>,
    const_overrides: &HashMap<String, String>,
    context: &str,
) -> Result<HashMap<String, Expr>> {
    let mut resolved_map: HashMap<String, Expr> = HashMap::new();

    for (name, value) in const_overrides {
        let Some(decl) = constants.iter_mut().find(|(n, _)| n == name) else {
            continue;
        };
        let expected = const_type_to_kind(&decl.1.const_type);
        decl.1.value = Some(const_cli_value_expr(value, expected, name)?);
    }

    let missing_values = missing_constant_values(constants);
    if !missing_values.is_empty() {
        bail!(
            "Missing {} constant values: {}",
            context,
            missing_values.join(", ")
        );
    }
    type_check_constant_declarations(constants, symbol_types)?;

    loop {
        let mut changed = false;
        for (name, decl) in constants.iter_mut() {
            if resolved_map.contains_key(name) {
                continue;
            }
            let value_expr = decl
                .value
                .as_mut()
                .expect("constants missing values are validated before resolution");

            let folded = fold_expr(value_expr, &resolved_map);
            if let Some(resolved_value) = fold_resolved_const_value(&decl.const_type, folded) {
                **value_expr = resolved_value.clone();
                resolved_map.insert(name.clone(), resolved_value);
                changed = true;
            }
        }

        if resolved_map.len() == constants.len() {
            break;
        }
        if !changed {
            let unresolved = constants
                .iter()
                .filter_map(|(name, _)| (!resolved_map.contains_key(name)).then_some(name.clone()))
                .collect::<Vec<_>>();
            bail!(
                "Unresolved {} constant declarations: {}",
                context,
                unresolved.join(", ")
            );
        }
    }

    Ok(resolved_map)
}

fn rename_expr(expr: &mut Expr, renames: &HashMap<String, String>) {
    match expr {
        Expr::BoolLit(_) | Expr::IntLit(_) | Expr::FloatLit(_) => {}
        Expr::LabelRef(_) => {}
        Expr::Ident(name) | Expr::PrimedIdent(name) => {
            if let Some(new_name) = renames.get(name) {
                *name = new_name.clone();
            }
        }
        Expr::UnaryOp { operand, .. } => rename_expr(operand.as_mut(), renames),
        Expr::BinOp { lhs, rhs, .. } => {
            rename_expr(lhs.as_mut(), renames);
            rename_expr(rhs.as_mut(), renames);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            rename_expr(cond.as_mut(), renames);
            rename_expr(then_branch.as_mut(), renames);
            rename_expr(else_branch.as_mut(), renames);
        }
    }
}

fn rename_box_expr(expr: &mut Box<Expr>, renames: &HashMap<String, String>) {
    rename_expr(expr.as_mut(), renames);
}

fn apply_module_renames(module: &mut Module, renames: &HashMap<String, String>) -> Result<()> {
    for var_decl in &mut module.local_vars {
        if let Some(new_name) = renames.get(&var_decl.name) {
            var_decl.name = new_name.clone();
        } else {
            bail!(format!(
                "Renamed module '{}' doesn't rename the local variable '{}'.",
                module.name, var_decl.name
            ));
        }

        match &mut var_decl.var_type {
            VarType::BoundedInt { lo, hi } => {
                rename_box_expr(lo, renames);
                rename_box_expr(hi, renames);
            }
            VarType::Bool => {}
        }

        rename_box_expr(&mut var_decl.init, renames);
    }

    for command in &mut module.commands {
        rename_box_expr(&mut command.guard, renames);
        for update in &mut command.updates {
            rename_box_expr(&mut update.prob, renames);
            for assignment in &mut update.assignments {
                rename_box_expr(assignment, renames);
            }
        }
    }
    Ok(())
}

fn expand_renamed_modules(model: &mut BasicAst) -> Result<()> {
    if model.renamed_modules.is_empty() {
        return Ok(());
    }

    let mut module_names: HashSet<String> = model.modules.iter().map(|m| m.name.clone()).collect();
    let renamed_modules = std::mem::take(&mut model.renamed_modules);

    for renamed in renamed_modules {
        if module_names.contains(&renamed.name) {
            bail!("Duplicate module declaration '{}'.", renamed.name);
        }

        let mut rename_map = HashMap::new();
        for (from, to) in &renamed.renames {
            if let Some(existing) = rename_map.insert(from.clone(), to.clone()) {
                // allows repeated renames as long as they don't conflict, e.g. [s1=s2, s1=s2] is fine
                if existing != *to {
                    bail!(
                        "Conflicting rename substitution for '{}' in module '{}'.",
                        from,
                        renamed.name
                    );
                }
            }
        }

        let base_module = model
            .modules
            .iter()
            .find(|m| m.name == renamed.base)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "Renamed module '{}' references unknown base module '{}'.",
                    renamed.name,
                    renamed.base
                )
            })?;

        let mut expanded = base_module;
        expanded.name = renamed.name.clone();
        apply_module_renames(&mut expanded, &rename_map)?;

        module_names.insert(expanded.name.clone());
        model.modules.push(expanded);
    }

    Ok(())
}

fn parse_const_overrides(s: &HashMap<String, String>) -> HashMap<String, String> {
    s.clone()
}

fn apply_and_resolve_constants(
    model: &mut BasicAst,
    symbol_types: &HashMap<String, TypeKind>,
    const_overrides: &HashMap<String, String>,
) -> Result<HashMap<String, Expr>> {
    apply_and_resolve_constants_for_decls(
        &mut model.constants,
        symbol_types,
        const_overrides,
        "model",
    )
}

trait PropertyExt {
    fn path_formula_mut(&mut self) -> &mut PathFormula;
}

impl PropertyExt for DTMCProperty {
    fn path_formula_mut(&mut self) -> &mut PathFormula {
        match self {
            DTMCProperty::ProbQuery(p) | DTMCProperty::RewardQuery(p) => p,
        }
    }
}

impl PropertyExt for MDPProperty {
    fn path_formula_mut(&mut self) -> &mut PathFormula {
        match self {
            MDPProperty::MaxProbQuery(p)
            | MDPProperty::MinProbQuery(p)
            | MDPProperty::MaxRewardQuery(p)
            | MDPProperty::MinRewardQuery(p) => p,
        }
    }
}

fn analyze_properties<P: PropertyExt>(
    properties: &mut [P],
    symbol_types: &HashMap<String, TypeKind>,
    constant_values: &HashMap<String, Expr>,
    labels_by_name: &HashMap<String, Box<Expr>>,
) -> Result<()> {
    for property in properties {
        let path = property.path_formula_mut();

        expand_label_refs_in_path_formula(path, labels_by_name)?;
        fold_path_formula(path, constant_values);
        type_check_path_formula(path, symbol_types)?;

        validate_path_formula_semantics(path, symbol_types)?;
    }

    Ok(())
}

fn validate_path_formula_semantics(
    path: &PathFormula,
    symbol_types: &HashMap<String, TypeKind>,
) -> Result<()> {
    match path {
        PathFormula::Next(phi) => {
            ensure_no_primed_idents(phi, "In X phi")?;
            ensure_type_ok(
                infer_expr_type(phi, symbol_types)? == TypeKind::Bool,
                "Path formula 'X phi' requires bool phi",
            )?;
        }
        PathFormula::Until { lhs, rhs, bound } => {
            analyze_temporal_op(lhs, rhs, bound, "until", symbol_types)?;
        }
        PathFormula::Release { lhs, rhs, bound } => {
            analyze_temporal_op(lhs, rhs, bound, "release", symbol_types)?;
        }
    }
    Ok(())
}

fn analyze_temporal_op(
    lhs: &Expr,
    rhs: &Expr,
    bound: &Option<Box<Expr>>,
    op_name: &str,
    symbol_types: &HashMap<String, TypeKind>,
) -> Result<()> {
    ensure_no_primed_idents(lhs, &format!("In {} lhs formula", op_name))?;
    ensure_no_primed_idents(rhs, &format!("In {} rhs formula", op_name))?;
    ensure_type_ok(
        infer_expr_type(lhs, symbol_types)? == TypeKind::Bool,
        format!("{} lhs formula must be bool", op_name),
    )?;
    ensure_type_ok(
        infer_expr_type(rhs, symbol_types)? == TypeKind::Bool,
        format!("{} rhs formula must be bool", op_name),
    )?;

    if let Some(k) = bound {
        ensure_no_primed_idents(k, &format!("In bounded {} bound expression", op_name))?;
        ensure_type_ok(
            infer_expr_type(k, symbol_types)? == TypeKind::Int,
            format!("Bounded-{} bound must have int type", op_name),
        )?;
        ensure_type_ok(
            matches!(k.as_ref(), Expr::IntLit(v) if *v >= 0),
            format!(
                "Bounded-{} bound must fold to a non-negative integer literal",
                op_name
            ),
        )?;
    }
    Ok(())
}

fn analyze_labels(
    labels: &mut [LabelDecl],
    symbol_types: &HashMap<String, TypeKind>,
    constant_values: &HashMap<String, Expr>,
) -> Result<HashMap<String, Box<Expr>>> {
    let mut labels_by_name = HashMap::new();
    for label in labels.iter() {
        if labels_by_name
            .insert(label.name.clone(), label.expr.clone())
            .is_some()
        {
            bail!("Duplicate label declaration '\"{}\"'.", label.name);
        }
    }

    for label in labels.iter_mut() {
        fold_box_expr(&mut label.expr, constant_values);
    }

    for label in labels.iter_mut() {
        expand_label_refs_in_box_expr(&mut label.expr, &labels_by_name)
            .map_err(|e| anyhow!("In label '\"{}\"': {}", label.name, e))?;
        type_check_expr(&label.expr, symbol_types)
            .map_err(|e| anyhow!("In label '\"{}\"': {}", label.name, e))?;
        ensure_no_primed_idents(&label.expr, &format!("In label '\"{}\"'", label.name))?;
        ensure_type_ok(
            infer_expr_type(&label.expr, symbol_types)? == TypeKind::Bool,
            format!("Label '\"{}\"' must have bool type", label.name),
        )?;
    }

    Ok(labels
        .iter()
        .map(|label| (label.name.clone(), label.expr.clone()))
        .collect())
}

/// Type-checks all state-formula expressions contained in a path formula.
///
/// This pass focuses only on expression typing and returns context-rich error messages.
fn type_check_path_formula(
    path: &PathFormula,
    symbol_types: &HashMap<String, TypeKind>,
) -> Result<()> {
    match path {
        PathFormula::Next(phi) => {
            type_check_expr(phi, symbol_types).map_err(|e| anyhow!("In X phi expression: {}", e))
        }
        PathFormula::Until { lhs, rhs, bound } => {
            type_check_expr(lhs, symbol_types)
                .map_err(|e| anyhow!("In until lhs expression: {}", e))?;
            type_check_expr(rhs, symbol_types)
                .map_err(|e| anyhow!("In until rhs expression: {}", e))?;
            if let Some(k) = bound {
                type_check_expr(k, symbol_types)
                    .map_err(|e| anyhow!("In bounded-until bound expression: {}", e))?;
            }
            Ok(())
        }
        PathFormula::Release { lhs, rhs, bound } => {
            type_check_expr(lhs, symbol_types)
                .map_err(|e| anyhow!("In release lhs expression: {}", e))?;
            type_check_expr(rhs, symbol_types)
                .map_err(|e| anyhow!("In release rhs expression: {}", e))?;
            if let Some(k) = bound {
                type_check_expr(k, symbol_types)
                    .map_err(|e| anyhow!("In bounded-release bound expression: {}", e))?;
            }
            Ok(())
        }
    }
}

/// Type-checks one expression against the current symbol table.
fn type_check_expr(expr: &Expr, symbol_types: &HashMap<String, TypeKind>) -> Result<()> {
    infer_expr_type(expr, symbol_types).map(|_| ())
}

/// Analyze and normalize a DTMC/MDP AST before symbolic translation.
///
/// This pass:
/// 1. expands renamed modules and builds a global symbol table,
/// 2. validates `--const` override keys,
/// 3. applies/validates/folds constant declarations,
/// 4. folds constants through variable declarations and checks bounds/init expressions,
/// 5. folds constants through commands and validates guards/probabilities/assignments,
/// 6. Expand labels, resolving nested labels
/// 7. folds constants and labels through properties and validates path formulas,
/// 8. inserts default actions for unlabeled commands and validates label usage,
/// 9. collects module/action/variable metadata for symbolic construction.
pub fn analyse_basic_model(
    model: &mut BasicAst,
    const_overrides: &HashMap<String, String>,
) -> Result<BasicModelInfo> {
    expand_renamed_modules(model)?;

    let const_overrides = parse_const_overrides(const_overrides);

    let mut symbol_types: HashMap<String, TypeKind> = HashMap::new();
    for (name, decl) in &model.constants {
        if symbol_types
            .insert(name.clone(), const_type_to_kind(&decl.const_type))
            .is_some()
        {
            bail!("Duplicate constant declaration '{}'.", name);
        }
    }

    for module in &model.modules {
        for var_decl in &module.local_vars {
            let kind = var_type_to_kind(&var_decl.var_type);
            if symbol_types.insert(var_decl.name.clone(), kind).is_some() {
                bail!("Duplicate symbol declaration '{}'.", var_decl.name);
            }
        }
    }

    for name in const_overrides.keys() {
        if !model.constants.iter().any(|(n, _)| n == name) {
            bail!("Unknown constant in --const override: '{}'", name);
        }
    }

    let constant_values = apply_and_resolve_constants(model, &symbol_types, &const_overrides)?;

    for module in &mut model.modules {
        for var_decl in &mut module.local_vars {
            let mut folded_bounds: Option<(i32, i32)> = None;
            match &mut var_decl.var_type {
                VarType::BoundedInt { lo, hi } => {
                    fold_box_expr(lo, &constant_values);
                    fold_box_expr(hi, &constant_values);
                    type_check_expr(lo, &symbol_types)
                        .map_err(|e| anyhow!("In lower bound of '{}': {}", var_decl.name, e))?;
                    type_check_expr(hi, &symbol_types)
                        .map_err(|e| anyhow!("In upper bound of '{}': {}", var_decl.name, e))?;
                    ensure_type_ok(
                        matches!(lo.as_ref(), Expr::IntLit(_))
                            && matches!(hi.as_ref(), Expr::IntLit(_)),
                        format!(
                            "Bounds of variable '{}' must fold to integer literals",
                            var_decl.name
                        ),
                    )?;

                    let (lo_val, hi_val) = match (lo.as_ref(), hi.as_ref()) {
                        (Expr::IntLit(lo_val), Expr::IntLit(hi_val)) => (*lo_val, *hi_val),
                        _ => unreachable!("bounds must be folded integer literals"),
                    };
                    ensure_type_ok(
                        lo_val <= hi_val,
                        format!(
                            "Invalid bounds for '{}': lower bound {} exceeds upper bound {}",
                            var_decl.name, lo_val, hi_val
                        ),
                    )?;
                    folded_bounds = Some((lo_val, hi_val));
                }
                VarType::Bool => {}
            }

            fold_box_expr(&mut var_decl.init, &constant_values);
            type_check_expr(&var_decl.init, &symbol_types)
                .map_err(|e| anyhow!("In init expression of '{}': {}", var_decl.name, e))?;

            let init_ty = infer_expr_type(&var_decl.init, &symbol_types)?;
            let decl_ty = var_type_to_kind(&var_decl.var_type);
            ensure_type_ok(
                init_ty == decl_ty,
                format!(
                    "Type error in init of '{}': expected {}, found {}",
                    var_decl.name,
                    decl_ty.as_str(),
                    init_ty.as_str()
                ),
            )?;

            if let Some((lo, hi)) = folded_bounds {
                let init_val = match var_decl.init.as_ref() {
                    Expr::IntLit(v) => *v,
                    _ => unreachable!("bounded int init must be an int literal after folding"),
                };
                ensure_type_ok(
                    init_val >= lo && init_val <= hi,
                    format!(
                        "Initial value of '{}' out of bounds: {} not in [{}..{}]",
                        var_decl.name, init_val, lo, hi
                    ),
                )?;
            }
        }

        for command in &mut module.commands {
            fold_box_expr(&mut command.guard, &constant_values);
            type_check_expr(&command.guard, &symbol_types)
                .map_err(|e| anyhow!("In guard of module '{}': {}", module.name, e))?;
            ensure_type_ok(
                infer_expr_type(&command.guard, &symbol_types)? == TypeKind::Bool,
                format!("Guard in module '{}' must have type bool", module.name),
            )?;

            for update in &mut command.updates {
                fold_box_expr(&mut update.prob, &constant_values);
                type_check_expr(&update.prob, &symbol_types)
                    .map_err(|e| anyhow!("In probability expression: {}", e))?;
                let prob_ty = infer_expr_type(&update.prob, &symbol_types)?;
                ensure_type_ok(
                    prob_ty == TypeKind::Int || prob_ty == TypeKind::Float,
                    "Probability expressions must be int or float",
                )?;

                for assignment in &mut update.assignments {
                    fold_box_expr(assignment, &constant_values);
                    type_check_expr(assignment, &symbol_types)
                        .map_err(|e| anyhow!("In assignment expression: {}", e))?;

                    if let Expr::BinOp {
                        lhs,
                        op: BinOp::Eq,
                        rhs,
                    } = assignment.as_ref()
                        && let Expr::PrimedIdent(name) = lhs.as_ref()
                    {
                        let lhs_ty = symbol_types
                            .get(name)
                            .copied()
                            .ok_or_else(|| anyhow!("Unknown assignment target '{}'", name))?;
                        let rhs_ty = infer_expr_type(rhs, &symbol_types)?;
                        ensure_type_ok(
                            lhs_ty == rhs_ty,
                            format!(
                                "Assignment type mismatch for '{}': lhs {}, rhs {}",
                                name,
                                lhs_ty.as_str(),
                                rhs_ty.as_str()
                            ),
                        )?;
                    }
                }
            }
        }
    }

    let _labels_by_name = analyze_labels(&mut model.labels, &symbol_types, &constant_values)?;

    let mut synchronisation_actions: HashMap<String, Vec<String>> = HashMap::new();
    let mut local_variables: HashMap<String, String> = HashMap::new();
    let mut var_bounds: HashMap<String, (i32, i32)> = HashMap::new();

    for module in &mut model.modules {
        let default_module_label = format!("__{}_action__", module.name);
        for command in &mut module.commands {
            if command.labels.is_empty() {
                command.labels.push(default_module_label.clone());
            } else if command.labels.len() == 1 {
                if command.labels[0] == default_module_label {
                    bail!(
                        "Explicit action label '{}' conflicts with default label for module '{}'. Please rename the action or the module.",
                        default_module_label,
                        module.name
                    );
                }
            } else {
                bail!(
                    "Multiple action labels on a single command are not supported: {:?}",
                    command.labels
                );
            }

            let action = &command.labels[0];
            if let Some(modules) = synchronisation_actions.get_mut(action) {
                if modules.last() != Some(&module.name) {
                    modules.push(module.name.clone());
                }
            } else {
                synchronisation_actions.insert(action.clone(), vec![module.name.clone()]);
            }
        }

        for var_decl in &module.local_vars {
            if local_variables.contains_key(&var_decl.name) {
                bail!(
                    "Local variable '{}' is declared in multiple modules: {:?} {:?}",
                    var_decl.name,
                    local_variables.get(&var_decl.name).unwrap(),
                    module.name
                );
            }
            local_variables.insert(var_decl.name.clone(), module.name.clone());
            match &var_decl.var_type {
                VarType::BoundedInt { lo, hi } => {
                    if let (Expr::IntLit(lo_val), Expr::IntLit(hi_val)) = (&**lo, &**hi) {
                        var_bounds.insert(var_decl.name.clone(), (*lo_val, *hi_val));
                    } else {
                        bail!(
                            "Bounds of variable '{}' must be integer literals: {:?} {:?}",
                            var_decl.name,
                            lo,
                            hi
                        );
                    }
                }
                VarType::Bool => {
                    var_bounds.insert(var_decl.name.clone(), (0, 1));
                }
            }
        }
    }

    Ok(BasicModelInfo {
        module_names: model.modules.iter().map(|m| m.name.clone()).collect(),
        modules_of_act: synchronisation_actions,
        module_of_var: local_variables,
        var_bounds,
    })
}
