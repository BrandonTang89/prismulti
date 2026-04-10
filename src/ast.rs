#[derive(Debug)]
pub struct DTMCAst {
    pub modules: Vec<Module>,
    // constants
    // global vars
    // functions, etc.
}

#[derive(Debug)]
pub struct Module {
    pub name: String,
    pub local_vars: Vec<VarDecl>,
    pub commands: Vec<Command>,
}

#[derive(Debug)]
pub struct VarDecl {
    pub name: String,
    pub var_type: VarType,
    pub init: Box<Expr>,
}

#[derive(Debug)]
pub enum VarType {
    BoundedInt { lo: Box<Expr>, hi: Box<Expr> },
    Bool,
}

#[derive(Debug)]
pub struct Command {
    pub labels: Vec<String>,
    pub guard: Box<Expr>,
    pub updates: Vec<ProbUpdate>,
}

#[derive(Debug)]
pub struct ProbUpdate {
    pub prob: Box<Expr>,
    pub assignments: Vec<Box<Expr>>,
}

#[derive(Debug)]
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

#[derive(Debug)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Debug)]
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
#[derive(Debug)]
pub struct RenamedModule {
    pub name: String,
    pub base: String,
    pub renames: Vec<(String, String)>,
}
