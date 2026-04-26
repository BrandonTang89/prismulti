use std::collections::HashMap;

use clap::{Parser, ValueEnum};
use prismulti::analyze::{analyse_dtmc, analyse_mdp};
use prismulti::ast::Expr;
use prismulti::parser::{parse_dtmc, parse_dtmc_props, parse_mdp, parse_mdp_props};
use prismulti::sym_check::{PropertyEvaluation, evaluate_property_at_initial_state};
use tracing::Level;
use tracing::{debug, info};
use tracing_subscriber::FmtSubscriber;

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ModelType {
    Dtmc,
    Mdp,
}

impl ModelType {
    fn name(self) -> &'static str {
        match self {
            ModelType::Dtmc => "DTMC",
            ModelType::Mdp => "MDP",
        }
    }
}

/// Command-line arguments.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    model_type: ModelType,

    #[arg(long)]
    model: String,

    #[arg(short, long)]
    verbose: bool,

    #[arg(long = "const")]
    const_values: Option<String>,

    #[arg(long)]
    prop_file: Option<String>,

    #[arg(long)]
    props: Option<String>,
}

fn parse_const_arg(input: &str) -> anyhow::Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    if input.trim().is_empty() {
        return Ok(map);
    }

    for pair in input.split(',') {
        let trimmed = pair.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            anyhow::bail!(
                "Invalid --const entry '{}'. Expected NAME=VALUE pairs separated by commas.",
                trimmed
            );
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() {
            anyhow::bail!("Invalid --const entry '{}': empty constant name.", trimmed);
        }
        map.insert(name.to_string(), value.to_string());
    }
    Ok(map)
}

fn parse_prop_indices_arg(input: &str, property_count: usize) -> anyhow::Result<Vec<usize>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut indices = Vec::new();
    for token in trimmed.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let idx = token
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Invalid --props entry '{}': expected integer", token))?;
        if idx == 0 {
            anyhow::bail!("Invalid --props entry '{}': indices are 1-based", token);
        }
        if idx > property_count {
            anyhow::bail!(
                "Invalid --props entry '{}': model has only {} properties",
                token,
                property_count
            );
        }
        indices.push(idx - 1);
    }

    Ok(indices)
}

fn print_analysis_summary<M: prismulti::ast::ModelKind>(
    ast: &prismulti::ast::Ast<M>,
    info: &prismulti::analyze::BasicModelInfo,
) {
    println!("Model analysis successful:");
    println!("  Module names: {:?}", info.module_names);
    println!("  Initial state:");
    for module in &ast.modules {
        for var_decl in &module.local_vars {
            let init_str = match var_decl.init.as_ref() {
                Expr::BoolLit(v) => v.to_string(),
                Expr::IntLit(v) => v.to_string(),
                Expr::FloatLit(v) => v.to_string(),
                other => format!("{other}"),
            };
            println!("    {} = {}", var_decl.name, init_str);
        }
    }
    if !ast.properties.is_empty() {
        println!("  Properties:");
        for (idx, prop) in ast.properties.iter().enumerate() {
            println!("    {}. {}", idx + 1, prop);
        }
    }
}

fn main() {
    const BANNER: &str = r#"
            _                     _ _   _ 
 _ __  _ __(_)___ _ __ ___  _   _| | |_(_)
| '_ \| '__| / __| '_ ` _ \| | | | | __| |
| |_) | |  | \__ \ | | | | | |_| | | |_| |
| .__/|_|  |_|___/_| |_| |_|\__,_|_|\__|_|
|_|                                                       
"#;
    println!("{BANNER}");

    let args = Args::parse();
    let const_overrides = match &args.const_values {
        Some(v) => match parse_const_arg(v) {
            Ok(map) => map,
            Err(e) => {
                eprintln!("Failed to parse --const: {e}");
                return;
            }
        },
        None => HashMap::new(),
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(if args.verbose {
            Level::DEBUG
        } else {
            Level::INFO
        })
        .without_time()
        .with_target(false)
        .with_file(false)
        .with_line_number(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    println!(
        "Parsing {} model from file: {}",
        args.model_type.name(),
        args.model
    );
    let model_str = match std::fs::read_to_string(&args.model) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to read model file: {e}");
            return;
        }
    };

    let props_str = if let Some(props_path) = &args.prop_file {
        println!("Parsing property file: {}", props_path);
        match std::fs::read_to_string(props_path) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("Failed to read property file: {e}");
                return;
            }
        }
    } else {
        None
    };

    match args.model_type {
        ModelType::Dtmc => {
            let mut ast = match parse_dtmc(&model_str) {
                Ok(ast) => ast,
                Err(e) => {
                    eprintln!("Failed to parse DTMC model: {e}");
                    return;
                }
            };
            if let Some(props_str) = props_str {
                match parse_dtmc_props(&props_str) {
                    Ok((mut prop_constants, mut properties)) => {
                        ast.constants.append(&mut prop_constants);
                        ast.properties.append(&mut properties);
                    }
                    Err(e) => {
                        eprintln!("Failed to parse DTMC property file: {e}");
                        return;
                    }
                }
            }

            let analyzed = analyse_dtmc(&mut ast, &const_overrides);
            let info = match analyzed {
                Ok(info) => {
                    print_analysis_summary(&ast, &info);
                    info
                }
                Err(e) => {
                    eprintln!("Model analysis failed: {e}");
                    return;
                }
            };

            let mut symbolic_dtmc = prismulti::constr_symbolic::build_symbolic_dtmc(ast, info);

            println!("Symbolic DTMC:\n  {}", symbolic_dtmc.describe().join("  "));

            if symbolic_dtmc.ast.properties.is_empty() {
                info!("No properties found; skipping model checking");
                return;
            }

            let selected = match &args.props {
                Some(indices) => {
                    match parse_prop_indices_arg(indices, symbolic_dtmc.ast.properties.len()) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("Failed to parse --props: {e}");
                            return;
                        }
                    }
                }
                None => (0..symbolic_dtmc.ast.properties.len()).collect(),
            };

            println!("Checking {} selected properties", selected.len());
            for &prop_idx in &selected {
                let prop_number = prop_idx + 1;
                let property = symbolic_dtmc.ast.properties[prop_idx].clone();
                info!("Checking property #{}: {}", prop_number, property);
                match evaluate_property_at_initial_state(&mut symbolic_dtmc, &property) {
                    Ok(PropertyEvaluation::Probability(value)) => {
                        println!("  {}. {} = {}", prop_number, property, value);
                    }
                    Ok(PropertyEvaluation::Unsupported(reason)) => {
                        println!("  {}. {} = unsupported ({})", prop_number, property, reason);
                    }
                    Err(e) => {
                        eprintln!("  {}. {} = error: {}", prop_number, property, e);
                    }
                }
                debug!("Finished property #{}", prop_number);
            }
        }
        ModelType::Mdp => {
            let mut ast = match parse_mdp(&model_str) {
                Ok(ast) => ast,
                Err(e) => {
                    eprintln!("Failed to parse MDP model: {e}");
                    return;
                }
            };
            if let Some(props_str) = props_str {
                match parse_mdp_props(&props_str) {
                    Ok((mut prop_constants, mut properties)) => {
                        ast.constants.append(&mut prop_constants);
                        ast.properties.append(&mut properties);
                    }
                    Err(e) => {
                        eprintln!("Failed to parse MDP property file: {e}");
                        return;
                    }
                }
            }

            let analyzed = analyse_mdp(&mut ast, &const_overrides);
            match analyzed {
                Ok(info) => {
                    print_analysis_summary(&ast, &info);
                }
                Err(e) => {
                    eprintln!("Model analysis failed: {e}");
                    return;
                }
            };

            info!(
                "MDP parsing and analysis complete; symbolic MDP construction is not implemented yet"
            );
        }
    }
}
