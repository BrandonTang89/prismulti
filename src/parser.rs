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
pub fn parse_dtmc(input: &str) -> Result<ast::DTMCAst, String> {
    let parser = parser::DTMCParser::new();
    parser.parse(input).map_err(|e| {
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
            _ => e.to_string(),
        };
        msg
    })
}

fn line_col(input: &str, byte_offset: usize) -> (usize, usize) {
    let line = input[..byte_offset].matches('\n').count() + 1;
    let col = byte_offset - input[..byte_offset].rfind('\n').map(|i| i + 1).unwrap_or(0) + 1;
    (line, col)
}
