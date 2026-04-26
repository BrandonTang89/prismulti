use prismulti::analyze::analyse_mdp;
use prismulti::parser::{parse_mdp, parse_mdp_props};
use std::collections::HashMap;

#[test]
fn test_parse_simple_mdp() {
    let model_str =
        std::fs::read_to_string("tests/mdp/simple_mdp.prism").expect("Failed to read model file");
    let mut ast = parse_mdp(&model_str).expect("Failed to parse MDP model");

    let prop_str =
        std::fs::read_to_string("tests/mdp/simple_mdp.prop").expect("Failed to read property file");
    let (mut prop_constants, mut properties) =
        parse_mdp_props(&prop_str).expect("Failed to parse MDP properties");

    ast.constants.append(&mut prop_constants);
    ast.properties.append(&mut properties);

    let _info = analyse_mdp(&mut ast, &HashMap::new()).expect("Failed to analyze MDP model");

    assert!(ast.properties.len() > 0);
}

#[test]
fn test_parse_robot_mdp() {
    let model_str =
        std::fs::read_to_string("tests/mdp/robot.prism").expect("Failed to read model file");
    let mut ast = parse_mdp(&model_str).expect("Failed to parse MDP model");

    let prop_str =
        std::fs::read_to_string("tests/mdp/robot.prop").expect("Failed to read property file");
    let (mut prop_constants, mut properties) =
        parse_mdp_props(&prop_str).expect("Failed to parse MDP properties");

    ast.constants.append(&mut prop_constants);
    ast.properties.append(&mut properties);

    let _info = analyse_mdp(&mut ast, &HashMap::new()).expect("Failed to analyze MDP model");
}
