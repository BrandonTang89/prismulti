pub mod utils;

/// Top-level DTMC AST.
#[derive(Clone, Debug)]
pub struct DTMCAst {
    pub modules: Vec<Module>,
    pub constants: Vec<(String, ConstDecl)>,
    pub renamed_modules: Vec<RenamedModule>,
    pub labels: Vec<LabelDecl>,
    pub properties: Vec<Property>,
    // global vars
    // functions, etc.
}

/// Label declaration.
#[derive(Clone, Debug)]
pub struct LabelDecl {
    pub name: String,
    pub expr: Box<Expr>,
}

/// Global constant declaration.
#[derive(Clone, Debug)]
pub struct ConstDecl {
    pub const_type: ConstType,
    pub value: Option<Box<Expr>>,
}

/// Supported constant types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstType {
    Bool,
    Int,
    Float,
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
    LabelRef(String),

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

/// Renamed module declaration
#[derive(Clone, Debug)]
pub struct RenamedModule {
    pub name: String,
    pub base: String,
    pub renames: Vec<(String, String)>,
}

/// Supported property query kinds.
#[derive(Clone, Debug)]
pub enum Property {
    ProbQuery(PathFormula),
    RewardQuery(PathFormula),
}

/// Supported path formulas for the current parser subset.
#[derive(Clone, Debug)]
pub enum PathFormula {
    Next(Box<Expr>),
    Until {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        bound: Option<Box<Expr>>,
    },
}

impl std::fmt::Display for UnOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnOp::Not => write!(f, "!"),
            UnOp::Neg => write!(f, "-"),
        }
    }
}

impl std::fmt::Display for BinOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BinOp::And => "&",
            BinOp::Or => "|",
            BinOp::Eq => "=",
            BinOp::Neq => "!=",
            BinOp::Lt => "<",
            BinOp::Leq => "<=",
            BinOp::Gt => ">",
            BinOp::Geq => ">=",
            BinOp::Plus => "+",
            BinOp::Minus => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
        };
        write!(f, "{s}")
    }
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::BoolLit(v) => write!(f, "{v}"),
            Expr::IntLit(v) => write!(f, "{v}"),
            Expr::FloatLit(v) => write!(f, "{v}"),
            Expr::Ident(name) => write!(f, "{name}"),
            Expr::PrimedIdent(name) => write!(f, "{name}'"),
            Expr::LabelRef(name) => write!(f, "\"{name}\""),
            Expr::UnaryOp { op, operand } => write!(f, "{}({})", op, operand),
            Expr::BinOp { lhs, op, rhs } => write!(f, "({} {} {})", lhs, op, rhs),
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => write!(f, "({} ? {} : {})", cond, then_branch, else_branch),
        }
    }
}

impl std::fmt::Display for PathFormula {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathFormula::Next(phi) => write!(f, "X {}", phi),
            PathFormula::Until { lhs, rhs, bound } => {
                if matches!(lhs.as_ref(), Expr::BoolLit(true)) && bound.is_none() {
                    write!(f, "F {}", rhs)
                } else if matches!(lhs.as_ref(), Expr::BoolLit(true)) {
                    write!(f, "F<={} {}", bound.as_ref().expect("bounded case"), rhs)
                } else if let Some(k) = bound {
                    write!(f, "{} U<={} {}", lhs, k, rhs)
                } else {
                    write!(f, "{} U {}", lhs, rhs)
                }
            }
        }
    }
}

impl std::fmt::Display for Property {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Property::ProbQuery(path) => write!(f, "P=? [{}]", path),
            Property::RewardQuery(path) => write!(f, "R=? [{}]", path),
        }
    }
}
