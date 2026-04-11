/// Top-level DTMC AST.
#[derive(Clone, Debug)]
pub struct DTMCAst {
    pub modules: Vec<Module>,
    // constants
    // global vars
    // functions, etc.
}

/// PRISM module declaration.
#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub local_vars: Vec<VarDecl>,
    pub commands: Vec<Command>,
}

/// Local variable declaration.
#[derive(Clone, Debug)]
pub struct VarDecl {
    pub name: String,
    pub var_type: VarType,
    pub init: Box<Expr>,
}

/// Supported variable types.
#[derive(Clone, Debug)]
pub enum VarType {
    BoundedInt { lo: Box<Expr>, hi: Box<Expr> },
    Bool,
}

/// Guarded command.
#[derive(Clone, Debug)]
pub struct Command {
    pub labels: Vec<String>,
    pub guard: Box<Expr>,
    pub updates: Vec<ProbUpdate>,
}

/// One probabilistic branch of a command update.
#[derive(Clone, Debug)]
pub struct ProbUpdate {
    pub prob: Box<Expr>,
    pub assignments: Vec<Box<Expr>>,
}

/// Expression language supported by the parser and symbolic translator.
#[derive(Clone, Debug)]
pub enum Expr {
    // Literals
    BoolLit(bool),
    IntLit(i32),
    FloatLit(f64),

    // References
    Ident(String),
    PrimedIdent(String),

    // Operators
    UnaryOp {
        op: UnOp,
        operand: Box<Expr>,
    },
    BinOp {
        lhs: Box<Expr>,
        op: BinOp,
        rhs: Box<Expr>,
    },
    Ternary {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
}

/// Unary operators.
#[derive(Clone, Debug)]
pub enum UnOp {
    Not,
    Neg,
}

/// Binary operators.
#[derive(Clone, Debug)]
pub enum BinOp {
    And,
    Or,
    Eq,
    Neq,
    Lt,
    Leq,
    Gt,
    Geq,
    Plus,
    Minus,
    Mul,
    Div,
}

/// `module mac2 = mac1 [s1=s2, s2=s1,...] endmodule`
#[derive(Clone, Debug)]
pub struct RenamedModule {
    pub name: String,
    pub base: String,
    pub renames: Vec<(String, String)>,
}
