use anyhow::Result;
use prismulti::constr_symbolic::build_symbolic_dtmc;
use prismulti::dd_manager::dd;
use prismulti::parser::parse_dtmc;
use prismulti::symbolic_dtmc::SymbolicDTMC;
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
    let info = prismulti::analyze::analyze_dtmc(&mut ast, const_overrides)?;
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
    let stats = dd::add_stats(
        symbolic_dtmc.transitions.get(),
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
fn dtmc_herman3_constr() {
    // regression + prism comparison test
    let const_overrides = HashMap::new();
    test_construction("tests/dtmc/herman3.prism", &const_overrides, 24, 3, 28, 8);
}

#[test]
fn dtmc_leader3_2_constr() {
    // regression + prism comparison test
    let const_overrides = HashMap::new();
    test_construction(
        "tests/dtmc/leader3_2.prism",
        &const_overrides,
        408,
        3,
        33,
        26,
    );
}

#[test]
fn dtmc_simple2_constr() {
    // regression + prism comparison test
    let const_overrides = HashMap::new();
    test_construction(
        "tests/dtmc/simple2_dtmc.prism",
        &const_overrides,
        26,
        4,
        9,
        6,
    );
}
