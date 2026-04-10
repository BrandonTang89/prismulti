/// Modifies the AST to assist in model checking.
/// Also gathers information about the model that will be useful for later stages of the pipeline
use crate::ast::*;
use anyhow::{Result, bail};

pub struct DTMCModelInfo {
    pub module_names: Vec<String>,

    /// action label -> Vec(modules with commands with this label)
    pub synchronisation_labels: std::collections::HashMap<String, Vec<String>>,

    /// LocalVarName -> ModuleName
    pub module_of_var: std::collections::HashMap<String, String>,

    /// VariableName -> (lo, hi)
    pub var_bounds: std::collections::HashMap<String, (i32, i32)>,
}

/// Adds explicit action labels to transitions that don't have them
/// todo: expand renamed-modules
pub fn analyze_dtmc(model: &mut DTMCAst) -> Result<DTMCModelInfo> {
    let mut synchronisation_labels: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut local_variables: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut var_bounds: std::collections::HashMap<String, (i32, i32)> =
        std::collections::HashMap::new();
    for module in &mut model.modules {
        let default_module_label = format!("__{}_action__", module.name);
        for commands in &mut module.commands {
            if commands.labels.is_empty() {
                commands.labels.push(default_module_label.clone());
            } else if commands.labels.len() == 1 {
                if commands.labels[0] == default_module_label {
                    bail!(
                        "Explicit action label '{}' conflicts with default label for module '{}'. Please rename the action or the module.",
                        default_module_label,
                        module.name
                    );
                }
            } else {
                bail!(
                    "Multiple action labels on a single command are not supported: {:?}",
                    commands.labels
                );
            }

            if synchronisation_labels.contains_key(&commands.labels[0]) {
                let modules = synchronisation_labels.get_mut(&commands.labels[0]).unwrap();
                // avoid duplicates
                if modules.last() != Some(&module.name) {
                    modules.push(module.name.clone());
                }
            } else {
                synchronisation_labels
                    .insert(commands.labels[0].clone(), vec![module.name.clone()]);
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

    Ok(DTMCModelInfo {
        module_names: model.modules.iter().map(|m| m.name.clone()).collect(),
        synchronisation_labels,
        module_of_var: local_variables,
        var_bounds,
    })
}
