use std::collections::HashMap;

use clap::{Parser, ValueEnum};
use prism_rs::parser::parse_dtmc;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[derive(ValueEnum, Clone, Debug)]
enum ModelType {
    DTMC,
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

fn main() {
    const BANNER: &str = r#"
            _                                  
 _ __  _ __(_)___ _ __ ___            _ __ ___ 
| '_ \| '__| / __| '_ ` _ \   _____  | '__/ __|
| |_) | |  | \__ \ | | | | | |_____| | |  \__ \
| .__/|_|  |_|___/_| |_| |_|         |_|  |___/
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
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    // Parse, analyze and construct the symbolic model for the selected type.
    match args.model_type {
        ModelType::DTMC => {
            println!("Parsing DTMC model from file: {}", args.model);
            let model_str =
                std::fs::read_to_string(&args.model).expect("Failed to read model file");

            let mut ast = match parse_dtmc(&model_str) {
                Ok(ast) => {
                    println!("Parsing successful");
                    ast
                }
                Err(e) => {
                    eprintln!("Failed to parse DTMC model: {e}");
                    return;
                }
            };
            let info = match prism_rs::analyze::analyze_dtmc(&mut ast, &const_overrides) {
                Ok(info) => {
                    println!("Model analysis successful:");
                    println!("  Module names: {:?}", info.module_names);
                    info
                }
                Err(e) => {
                    eprintln!("Model analysis failed: {e}");
                    return;
                }
            };

            let mut symbolic_dtmc = prism_rs::constr_symbolic::build_symbolic_dtmc(ast, info);

            println!("Symbolic DTMC:\n  {}", symbolic_dtmc.describe().join("  "));
        }
    }
}
