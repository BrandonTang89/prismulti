use std::collections::HashMap;

use prismulti::ast::{ConstType, DTMCProperty, Expr, PathFormula};
use prismulti::parser::{parse_dtmc, parse_dtmc_props};

#[test]
fn parses_interspersed_const_declarations() {
    let model = r#"
dtmc

const int N = 10;

module a
endmodule

const int M;
const bool x;
const float p = 0.25;

module b
endmodule
"#;

    let ast = parse_dtmc(model).expect("parse failed");

    assert_eq!(ast.modules.len(), 2);
    assert_eq!(ast.modules[0].name, "a");
    assert_eq!(ast.modules[1].name, "b");

    assert_eq!(ast.constants.len(), 4);

    assert_eq!(ast.constants[0].0, "N");
    assert_eq!(ast.constants[0].1.const_type, ConstType::Int);
    assert!(ast.constants[0].1.value.is_some());

    assert_eq!(ast.constants[1].0, "M");
    assert_eq!(ast.constants[1].1.const_type, ConstType::Int);
    assert!(ast.constants[1].1.value.is_none());

    assert_eq!(ast.constants[2].0, "x");
    assert_eq!(ast.constants[2].1.const_type, ConstType::Bool);
    assert!(ast.constants[2].1.value.is_none());

    assert_eq!(ast.constants[3].0, "p");
    assert_eq!(ast.constants[3].1.const_type, ConstType::Float);
    assert!(ast.constants[3].1.value.is_some());
}

#[test]
fn parses_const_declaration_forms() {
    let model = r#"
dtmc
const int N = 10;
const int M;
const bool x;
module m
endmodule
"#;

    let ast = parse_dtmc(model).expect("parse failed");
    assert_eq!(ast.constants.len(), 3);
}

#[test]
fn parses_and_expands_herman3_renamed_modules() {
    let model = std::fs::read_to_string("tests/dtmc/herman3.prism").expect("read failed");
    let mut ast = parse_dtmc(&model).expect("parse failed");

    assert_eq!(ast.modules.len(), 1);
    assert_eq!(ast.renamed_modules.len(), 2);

    let info =
        prismulti::analyze::analyse_dtmc(&mut ast, &HashMap::new()).expect("analysis failed");

    assert_eq!(ast.modules.len(), 3);
    assert!(ast.renamed_modules.is_empty());
    assert_eq!(info.module_names.len(), 3);
}

#[test]
fn parses_knuth_two_dice_prop_file() {
    let props = std::fs::read_to_string("tests/dtmc/knuth_two_dice.prop").expect("read failed");
    let (constants, properties) = parse_dtmc_props(&props).expect("parse failed");

    assert_eq!(constants.len(), 1);
    assert_eq!(constants[0].0, "x");
    assert_eq!(properties.len(), 2);

    match &properties[0] {
        DTMCProperty::ProbQuery(PathFormula::Until { lhs, rhs: _, bound }) => {
            assert!(matches!(lhs.as_ref(), Expr::BoolLit(true)));
            assert!(bound.is_none());
        }
        other => panic!("unexpected first property: {other:?}"),
    }

    match &properties[1] {
        DTMCProperty::RewardQuery(PathFormula::Until { lhs, rhs: _, bound }) => {
            assert!(matches!(lhs.as_ref(), Expr::BoolLit(true)));
            assert!(bound.is_none());
        }
        other => panic!("unexpected second property: {other:?}"),
    }
}

#[test]
fn parses_knuth_die_prop_file() {
    let props = std::fs::read_to_string("tests/dtmc/knuth_die.prop").expect("read failed");
    let (constants, properties) = parse_dtmc_props(&props).expect("parse failed");

    assert_eq!(constants.len(), 1);
    assert_eq!(constants[0].0, "x");
    assert_eq!(properties.len(), 3);

    assert!(matches!(
        properties[0],
        DTMCProperty::ProbQuery(PathFormula::Until { .. })
    ));
    assert!(matches!(
        properties[1],
        DTMCProperty::ProbQuery(PathFormula::Next(_))
    ));
    match &properties[2] {
        DTMCProperty::ProbQuery(PathFormula::Until {
            lhs: _,
            rhs: _,
            bound,
        }) => {
            assert!(bound.is_some());
        }
        other => panic!("unexpected third property: {other:?}"),
    }
}

#[test]
fn parses_leader_prop_file_with_label_and_bounded_finally() {
    let props = std::fs::read_to_string("tests/dtmc/leader.prop").expect("read failed");
    let (constants, properties) = parse_dtmc_props(&props).expect("parse failed");

    assert_eq!(constants.len(), 1);
    assert_eq!(constants[0].0, "L");
    assert_eq!(properties.len(), 4);

    match &properties[0] {
        DTMCProperty::ProbQuery(PathFormula::Until { lhs, rhs, bound }) => {
            assert!(matches!(lhs.as_ref(), Expr::BoolLit(true)));
            assert!(matches!(rhs.as_ref(), Expr::LabelRef(name) if name == "elected"));
            assert!(bound.is_none());
        }
        other => panic!("unexpected first property: {other:?}"),
    }

    match &properties[1] {
        DTMCProperty::ProbQuery(PathFormula::Until { lhs, rhs, bound }) => {
            assert!(matches!(lhs.as_ref(), Expr::BoolLit(true)));
            assert!(matches!(rhs.as_ref(), Expr::LabelRef(name) if name == "elected"));
            assert!(bound.is_some());
        }
        other => panic!("unexpected second property: {other:?}"),
    }

    match &properties[2] {
        DTMCProperty::RewardQuery(PathFormula::Until { lhs, rhs, bound }) => {
            assert!(matches!(lhs.as_ref(), Expr::BoolLit(true)));
            assert!(matches!(rhs.as_ref(), Expr::LabelRef(name) if name == "elected"));
            assert!(bound.is_none());
        }
        other => panic!("unexpected third property: {other:?}"),
    }

    match &properties[3] {
        DTMCProperty::ProbQuery(PathFormula::Release { lhs, rhs, bound }) => {
            assert!(matches!(rhs.as_ref(), Expr::BoolLit(false)));
            assert!(bound.is_none());
            // Globally (! "elected") -> (! "elected") R false
            match lhs.as_ref() {
                Expr::UnaryOp {
                    op: prismulti::ast::UnOp::Not,
                    operand,
                } => {
                    assert!(matches!(operand.as_ref(), Expr::LabelRef(name) if name == "elected"));
                }
                other => panic!("unexpected lhs in fourth property: {other:?}"),
            }
        }
        other => panic!("unexpected fourth property: {other:?}"),
    }
}
