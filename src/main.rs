use clap::{Parser, ValueEnum};
use prism_rs::parser::parse_dtmc;

#[derive(ValueEnum, Clone, Debug)]
enum ModelType {
    DTMC,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    model_type: ModelType,

    #[arg(long)]
    model: String,

    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    let args = Args::parse();
    println!("== Prism-rs ==");

    match args.model_type {
        ModelType::DTMC => {
            println!("Parsing DTMC model from file: {}", args.model);
            let model_str =
                std::fs::read_to_string(&args.model).expect("Failed to read model file");

            let mut ast = match parse_dtmc(&model_str) {
                Ok(ast) => {
                    println!("Successfully parsed DTMC model:");
                    ast
                }
                Err(e) => {
                    eprintln!("Failed to parse DTMC model: {e}");
                    return;
                }
            };
            let info = match prism_rs::analyze::analyze_dtmc(&mut ast) {
                Ok(info) => {
                    println!("Model analysis successful:");
                    println!("Module names: {:?}", info.module_names);
                    info
                }
                Err(e) => {
                    eprintln!("Model analysis failed: {e}");
                    return;
                }
            };

            let symbolic_dtmc = prism_rs::constr_symbolic::build_symbolic_dtmc(&ast, &info);
            println!("Symbolic DTMC construction successful:");
            println!("Transitions BDD node ID: {:?}", symbolic_dtmc.transitions);
        }
    }
}
