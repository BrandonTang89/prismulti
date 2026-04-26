use anyhow::Result;
use prismulti::analyze::analyse_dtmc;
use prismulti::parser::{parse_dtmc, parse_dtmc_props};
use prismulti::sym_check::{PropertyEvaluation, evaluate_property_at_initial_state};
use prismulti::symbolic_dtmc::SymbolicDTMC;
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

    let info = analyse_dtmc(&mut ast, const_overrides)?;
    Ok(prismulti::constr_symbolic::build_symbolic_dtmc(ast, info))
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

#[test]
fn dtmc_knuth_die_unbounded_until_property_probability() {
    for x in 1..=6 {
        let mut const_overrides = HashMap::new();
        const_overrides.insert("x".to_string(), x.to_string());
        let mut dtmc = construct_symbolic_dtmc_with_props(
            "tests/dtmc/knuth_die.prism",
            "tests/dtmc/knuth_die.prop",
            &const_overrides,
        )
        .expect("Failed to construct symbolic DTMC with properties");

        let property = {
            let properties = dtmc.ast.properties.clone();
            properties[0].clone()
        };

        match evaluate_property_at_initial_state(&mut dtmc, &property)
            .expect("Property checking failed")
        {
            PropertyEvaluation::Probability(value) => assert_close(value, 1.0 / 6.0, 1e-10),
            PropertyEvaluation::Unsupported(reason) => {
                panic!("Expected probability, got {reason}")
            }
        }
    }
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
}

#[test]
fn dtmc_knuth_two_dice_reachability_probability() {
    let cases = [
        (4, 0.0833333320915699_f64),
        (3, 0.0555555522441864_f64),
        (2, 0.0277777761220932_f64),
        (1, 0.0_f64),
    ];

    for (x, expected) in cases {
        let mut const_overrides = HashMap::new();
        const_overrides.insert("x".to_string(), x.to_string());
        let mut dtmc = construct_symbolic_dtmc_with_props(
            "tests/dtmc/knuth_two_dice.prism",
            "tests/dtmc/knuth_two_dice.prop",
            &const_overrides,
        )
        .expect("Failed to construct symbolic DTMC with properties");

        let property = {
            let properties = dtmc.ast.properties.clone();
            properties[0].clone()
        };

        match evaluate_property_at_initial_state(&mut dtmc, &property)
            .expect("Property checking failed")
        {
            PropertyEvaluation::Probability(value) => assert_close(value, expected, 5e-9),
            PropertyEvaluation::Unsupported(reason) => {
                panic!("Expected probability, got {reason}")
            }
        }
    }
}

#[test]
fn dtmc_brp_property_probabilities_with_constants() {
    let mut const_overrides = HashMap::new();
    const_overrides.insert("N".to_string(), "2".to_string());
    const_overrides.insert("MAX".to_string(), "3".to_string());

    let expected_probabilities = [
        0.0_f64,
        0.0_f64,
        1.5772293325415632e-6_f64,
        7.886142909415634e-7_f64,
        0.0_f64,
        1.6000000000000003e-7_f64,
    ];

    let mut dtmc = construct_symbolic_dtmc_with_props(
        "tests/dtmc/brp.prism",
        "tests/dtmc/brp.prop",
        &const_overrides,
    )
    .expect("Failed to construct symbolic DTMC with properties");

    for (idx, expected) in expected_probabilities.iter().enumerate() {
        let property = {
            let properties = dtmc.ast.properties.clone();
            properties[idx].clone()
        };

        match evaluate_property_at_initial_state(&mut dtmc, &property)
            .expect("Property checking failed")
        {
            PropertyEvaluation::Probability(value) => {
                assert_close(value, *expected, 1e-10);
            }
            PropertyEvaluation::Unsupported(reason) => {
                panic!("Expected probability, got {reason}")
            }
        }
    }
}

#[test]
fn dtmc_leader3_2_properties_with_constants() {
    let mut const_overrides = HashMap::new();
    const_overrides.insert("L".to_string(), "3".to_string());

    let mut dtmc = construct_symbolic_dtmc_with_props(
        "tests/dtmc/leader3_2.prism",
        "tests/dtmc/leader.prop",
        &const_overrides,
    )
    .expect("Failed to construct symbolic DTMC with properties");

    let properties = dtmc.ast.properties.clone();
    assert_eq!(properties.len(), 4);

    match evaluate_property_at_initial_state(&mut dtmc, &properties[0])
        .expect("Property checking failed")
    {
        PropertyEvaluation::Probability(value) => assert_close(value, 1.0, 1e-10),
        PropertyEvaluation::Unsupported(reason) => panic!("Expected probability, got {reason}"),
    }

    match evaluate_property_at_initial_state(&mut dtmc, &properties[1])
        .expect("Property checking failed")
    {
        PropertyEvaluation::Probability(value) => assert_close(value, 0.984375, 1e-10),
        PropertyEvaluation::Unsupported(reason) => panic!("Expected probability, got {reason}"),
    }

    match evaluate_property_at_initial_state(&mut dtmc, &properties[2])
        .expect("Property checking failed")
    {
        PropertyEvaluation::Unsupported(reason) => {
            assert_eq!(reason, "Reward properties are not supported yet")
        }
        PropertyEvaluation::Probability(value) => {
            panic!("Expected unsupported reward property, got probability {value}")
        }
    }

    // match evaluate_property_at_initial_state(&mut dtmc, &properties[3])
    //     .expect("Property checking failed")
    // {
    //     PropertyEvaluation::Probability(value) => assert_close(value, 0.0, 1e-10),
    //     PropertyEvaluation::Unsupported(reason) => panic!("Expected probability, got {reason}"),
    // }
}
