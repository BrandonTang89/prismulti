use anyhow::Result;
use prism_rs::analyze::analyze_dtmc;
use prism_rs::parser::{parse_dtmc, parse_dtmc_props};
use prism_rs::sym_check::{PropertyEvaluation, evaluate_property_at_initial_state};
use prism_rs::symbolic_dtmc::{RefLeakReport, SymbolicDTMC};
use std::collections::HashMap;

fn read_file(path: &str) -> Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

fn construct_symbolic_dtmc_with_props(
    model_path: &str,
    prop_path: &str,
    const_overrides: &HashMap<String, String>,
) -> Result<SymbolicDTMC> {
    let model_str = read_file(model_path)?;
    let prop_str = read_file(prop_path)?;

    let mut ast = parse_dtmc(&model_str)?;
    let (mut prop_constants, mut properties) = parse_dtmc_props(&prop_str)?;
    ast.constants.append(&mut prop_constants);
    ast.properties.append(&mut properties);

    let info = analyze_dtmc(&mut ast, const_overrides)?;
    Ok(prism_rs::constr_symbolic::build_symbolic_dtmc(ast, info))
}

fn assert_close(actual: f64, expected: f64, eps: f64) {
    assert!(
        (actual - expected).abs() <= eps,
        "Expected {} but got {} (eps={})",
        expected,
        actual,
        eps
    );
}

fn assert_zero_refs(report: RefLeakReport) {
    assert_eq!(
        report.nonzero_ref_count, 0,
        "Expected zero non-zero refs, got {}.",
        report.nonzero_ref_count
    );
}

#[test]
fn dtmc_knuth_die_next_property_probability() {
    let mut const_overrides = HashMap::new();
    const_overrides.insert("x".to_string(), "1".to_string());
    let mut dtmc = construct_symbolic_dtmc_with_props(
        "tests/dtmc/knuth_die.prism",
        "tests/dtmc/knuth_die.prop",
        &const_overrides,
    )
    .expect("Failed to construct symbolic DTMC with properties");

    let property = {
        let properties = dtmc.ast.properties.clone();
        properties[1].clone()
    };
    match evaluate_property_at_initial_state(&mut dtmc, &property)
        .expect("Property checking failed")
    {
        PropertyEvaluation::Probability(value) => assert_close(value, 0.5, 1e-10),
        PropertyEvaluation::Unsupported(reason) => panic!("Expected probability, got {reason}"),
    }

    assert_zero_refs(dtmc.release_report());
}

#[test]
fn dtmc_knuth_die_bounded_until_property_probability() {
    let mut const_overrides = HashMap::new();
    const_overrides.insert("x".to_string(), "1".to_string());
    let mut dtmc = construct_symbolic_dtmc_with_props(
        "tests/dtmc/knuth_die.prism",
        "tests/dtmc/knuth_die.prop",
        &const_overrides,
    )
    .expect("Failed to construct symbolic DTMC with properties");

    let property = {
        let properties = dtmc.ast.properties.clone();
        properties[2].clone()
    };
    match evaluate_property_at_initial_state(&mut dtmc, &property)
        .expect("Property checking failed")
    {
        PropertyEvaluation::Probability(value) => assert_close(value, 0.75, 1e-10),
        PropertyEvaluation::Unsupported(reason) => panic!("Expected probability, got {reason}"),
    }

    assert_zero_refs(dtmc.release_report());
}
