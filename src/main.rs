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
}

fn main() {
    let args = Args::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    println!("== Prism-rs ==");

    // Parse, analyze and construct the symbolic model for the selected type.
    match args.model_type {
        ModelType::DTMC => {
            println!("Parsing DTMC model from file: {}", args.model);
            let model_str =
                std::fs::read_to_string(&args.model).expect("Failed to read model file");

            let mut ast = match parse_dtmc(&model_str) {
                Ok(ast) => {
                    println!("Successfully parsed DTMC model");
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

            let mut symbolic_dtmc = prism_rs::constr_symbolic::build_symbolic_dtmc(ast, info);

            println!("Filtered DTMC:\n{}", symbolic_dtmc.describe());
        }
    }
}
