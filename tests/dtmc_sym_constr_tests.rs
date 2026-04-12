use anyhow::Result;
use prism_rs::constr_symbolic::build_symbolic_dtmc;
use prism_rs::parser::parse_dtmc;
use prism_rs::symbolic_dtmc::{RefLeakReport, SymbolicDTMC};
use std::collections::HashMap;

fn read_model_file(path: &str) -> Result<String> {
    let model_str = std::fs::read_to_string(path)?;
    Ok(model_str)
}

fn construct_symbolic_dtmc(
    model_str: &str,
    const_overrides: &HashMap<String, String>,
) -> Result<SymbolicDTMC> {
    let mut ast = parse_dtmc(model_str)?;
    let info = prism_rs::analyze::analyze_dtmc(&mut ast, const_overrides)?;
    let symbolic_dtmc = build_symbolic_dtmc(ast, info);
    Ok(symbolic_dtmc)
}

fn test_construction(
    model_path: &str,
    const_overrides: &HashMap<String, String>,
    expected_node_count: usize,
    expected_terminal_count: usize,
    expected_minterms: u64,
    expected_reachable_states: u64,
) {
    let model_str = read_model_file(model_path).expect("Failed to read model file");
    let mut symbolic_dtmc = construct_symbolic_dtmc(&model_str, const_overrides)
        .expect("Failed to construct symbolic DTMC");
    let stats = symbolic_dtmc.mgr.add_stats(
        symbolic_dtmc.transitions,
        symbolic_dtmc.total_variable_count() as u32,
    );

    assert_eq!(
        stats.node_count, expected_node_count,
        "Expected {} nodes in transition ADD, got {}",
        expected_node_count, stats.node_count
    );

    assert_eq!(
        stats.terminal_count, expected_terminal_count,
        "Expected {} terminals in transition ADD, got {}",
        expected_terminal_count, stats.terminal_count
    );

    assert_eq!(
        stats.minterms, expected_minterms,
        "Expected {} minterms in transition ADD, got {}",
        expected_minterms, stats.minterms
    );

    let num_reachable = symbolic_dtmc.reachable_state_count();
    assert_eq!(
        num_reachable, expected_reachable_states,
        "Expected {} reachable states, got {}",
        expected_reachable_states, num_reachable
    );

    assert_zero_refs(symbolic_dtmc.release_report());
}

fn assert_zero_refs(report: RefLeakReport) {
    assert_eq!(
        report.nonzero_ref_count, 0,
        "Expected zero non-zero refs, got {}. Entries: {:?}",
        report.nonzero_ref_count, report.nonzero_ref_entries
    );
}

#[test]
fn dtmc_simple_constr() {
    // regression test
    let const_overrides = HashMap::new();
    test_construction("tests/dtmc/simple_dtmc.prism", &const_overrides, 9, 2, 6, 3);
}

#[test]
fn dtmc_knuth_die_constr() {
    // regression + prism comparison test
    let const_overrides = HashMap::new();
    test_construction(
        "tests/dtmc/knuth_die.prism",
        &const_overrides,
        65,
        3,
        20,
        13,
    );
}

#[test]
fn dtmc_brp_constr() {
    let mut const_overrides = HashMap::new();
    const_overrides.insert("N".to_string(), "1".to_string());
    const_overrides.insert("MAX".to_string(), "2".to_string());

    // prism comparison test for: -const N=1,MAX=2 -mtbdd
    // We match PRISM's reachable states and transition minterms; DD node count is implementation-dependent.
    test_construction("tests/dtmc/brp.prism", &const_overrides, 320, 4, 23, 20);
}

#[test]
fn dtmc_knuth_two_dice_constr() {
    // regression + prism comparison test
    let const_overrides = HashMap::new();
    test_construction(
        "tests/dtmc/knuth_two_dice.prism",
        &const_overrides,
        274,
        3,
        79,
        45,
    );
}
