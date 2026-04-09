/// Modifies the AST to assist in model checking.
/// Also gathers information about the model that will be useful for later stages of the pipeline
use crate::ast::*;
use anyhow::{Result, bail};

pub struct DTMCModelInfo {
    pub module_names: Vec<String>,

    /// Map from action labels to the modules that they are present in
    pub synchronisation_labels: std::collections::HashMap<String, Vec<String>>,
}

/// Adds explicit action labels to transitions that don't have them
/// todo: expand renamed-modules
pub fn analyze_dtmc(model: &mut DTMCAst) -> Result<DTMCModelInfo> {
    let mut synchronisation_labels: std::collections::HashMap<String, Vec<String>> =
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
                synchronisation_labels
                    .get_mut(&commands.labels[0])
                    .unwrap()
                    .push(module.name.clone());
            } else {
                synchronisation_labels
                    .insert(commands.labels[0].clone(), vec![module.name.clone()]);
            }
        }
    }

    Ok(DTMCModelInfo {
        module_names: model.modules.iter().map(|m| m.name.clone()).collect(),
        synchronisation_labels,
    })
}
