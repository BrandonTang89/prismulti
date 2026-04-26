use anyhow::Result;
use lalrpop_util::lalrpop_mod;

use crate::ast;
lalrpop_mod!(
    #[allow(clippy::all)]
    #[rustfmt::skip]
    parser,
    "/parser/parser.rs"
);

/// Parse a PRISM DTMC model string into the project AST.
///
/// On parse failure, this reports line/column-oriented diagnostics to make
/// grammar errors easier to locate.
pub fn parse_dtmc(input: &str) -> Result<ast::DTMCAst> {
    parse_with(input, parser::DTMCParser::new().parse(input))
}

/// Parse a PRISM MDP model string into the project AST.
///
/// On parse failure, this reports line/column-oriented diagnostics to make
/// grammar errors easier to locate.
pub fn parse_mdp(input: &str) -> Result<ast::MDPAst> {
    parse_with(input, parser::MDPParser::new().parse(input))
}

/// Parsed DTMC property file payload: `(const_declarations, properties)`.
pub type ParsedDTMCProps = (Vec<(String, ast::ConstDecl)>, Vec<ast::DTMCProperty>);

/// Parsed MDP property file payload: `(const_declarations, properties)`.
pub type ParsedMDPProps = (Vec<(String, ast::ConstDecl)>, Vec<ast::MDPProperty>);

/// Parse a DTMC property file into property/query AST.
pub fn parse_dtmc_props(input: &str) -> Result<ParsedDTMCProps> {
    parse_with(input, parser::DTMCPropsParser::new().parse(input))
}

/// Parse an MDP property file into property/query AST.
pub fn parse_mdp_props(input: &str) -> Result<ParsedMDPProps> {
    parse_with(input, parser::MDPPropsParser::new().parse(input))
}

fn parse_with<T, Tok, Err>(
    input: &str,
    parse_result: std::result::Result<T, lalrpop_util::ParseError<usize, Tok, Err>>,
) -> Result<T>
where
    Tok: std::fmt::Display,
    Err: std::fmt::Display,
{
    parse_result.map_err(|e| parse_error_to_anyhow(input, e))
}

fn parse_error_to_anyhow<Tok, Err>(
    input: &str,
    e: lalrpop_util::ParseError<usize, Tok, Err>,
) -> anyhow::Error
where
    Tok: std::fmt::Display,
    Err: std::fmt::Display,
{
    let msg = match &e {
        lalrpop_util::ParseError::InvalidToken { location } => {
            let (line, col) = line_col(input, *location);
            format!("Invalid token at line {line}, col {col}")
        }
        lalrpop_util::ParseError::UnrecognizedToken {
            token: (start, tok, _),
            expected,
        } => {
            let (line, col) = line_col(input, *start);
            format!(
                "Unexpected token '{tok}' at line {line}, col {col}. Expected one of: {}",
                expected.join(", ")
            )
        }
        lalrpop_util::ParseError::UnrecognizedEof { expected, .. } => {
            format!(
                "Unexpected end of input. Expected one of: {}",
                expected.join(", ")
            )
        }
        lalrpop_util::ParseError::ExtraToken {
            token: (start, tok, _),
        } => {
            let (line, col) = line_col(input, *start);
            format!("Extra token '{tok}' at line {line}, col {col}")
        }
        lalrpop_util::ParseError::User { error } => format!("Parse error: {error}"),
    };
    anyhow::anyhow!(msg)
}

fn line_col(input: &str, byte_offset: usize) -> (usize, usize) {
    let line = input[..byte_offset].matches('\n').count() + 1;
    let col = byte_offset - input[..byte_offset].rfind('\n').map(|i| i + 1).unwrap_or(0) + 1;
    (line, col)
}
