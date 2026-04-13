use std::collections::HashMap;

use prism_rs::analyze::analyze_dtmc;
use prism_rs::ast::ConstType;
use prism_rs::parser::parse_dtmc;

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

    let info = analyze_dtmc(&mut ast, &HashMap::new()).expect("analysis failed");

    assert_eq!(ast.modules.len(), 3);
    assert!(ast.renamed_modules.is_empty());
    assert_eq!(info.module_names.len(), 3);
}
