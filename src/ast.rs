//! AST for the TypeScript subset.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Number,
    Boolean,
    Void,
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Number => write!(f, "number"),
            Type::Boolean => write!(f, "boolean"),
            Type::Void => write!(f, "void"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
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
    Num(f64),
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

pub struct Program {
    pub stmts: Vec<Stmt>,
}
