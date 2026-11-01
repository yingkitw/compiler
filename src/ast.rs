//! AST for the TypeScript subset.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Number,
    /// 64-bit signed integer. Distinct from `Number` so integer arithmetic
    /// stays exact and compiles to integer instructions.
    Int,
    Boolean,
    Void,
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Number => write!(f, "number"),
            Type::Int => write!(f, "int"),
            Type::Boolean => write!(f, "boolean"),
            Type::Void => write!(f, "void"),
        }
    }
}

/// A numeric literal.
///
/// The literal's text is parsed into both a `f64` and — when it is written as
/// a plain integer that fits in an `i64` — an exact `i64`. Keeping the exact
/// integer matters: `9007199254740993` (2^53 + 1) is not representable as an
/// `f64`, so parsing it as a double first would silently change the number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumLit {
    /// The value as a double. May have lost precision for large integers.
    pub value: f64,
    /// The exact value, when the literal is an integer that fits in an `i64`.
    pub int: Option<i64>,
}

impl NumLit {
    pub fn new(value: f64, int: Option<i64>) -> Self {
        Self { value, int }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    /// Remainder. Integer operands use `srem`; float operands go through the
    /// runtime's `ts_fmod`, because Cranelift has no float remainder.
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Num(NumLit),
    Bool(bool),
    Var(String),
    Assign { name: String, value: Box<Expr> },
    Bin { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Un { op: UnOp, expr: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<Type>,
        init: Option<Expr>,
        mutable: bool,
    },
    Func {
        name: String,
        params: Vec<(String, Type)>,
        ret: Option<Type>,
        body: Vec<Stmt>,
    },
    If {
        cond: Expr,
        then: Vec<Stmt>,
        els: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Block(Vec<Stmt>),
    Return(Option<Expr>),
    Expr(Expr),
}

#[derive(Debug)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}
