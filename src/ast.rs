#[derive(Debug, Clone)]
pub enum Stmt {
    VarDecl  { name: String, value: Expr, line: usize },
    Assign   { name: String, value: Expr, line: usize },
    Set      { object: Expr, name: String, value: Expr, line: usize },
    IndexSet { object: Expr, index: Expr, value: Expr, line: usize },
    Bring    { module: String, line: usize },
    FuncDef  { name: String, params: Vec<String>, body: Vec<Stmt>, line: usize },
    ClassDef { name: String, methods: Vec<Stmt>, line: usize },
    If {
        condition:      Expr,
        then_branch:    Vec<Stmt>,
        elif_branches:  Vec<(Expr, Vec<Stmt>)>,
        else_branch:    Option<Vec<Stmt>>,
        line:           usize,
    },
    While    { condition: Expr, body: Vec<Stmt>, line: usize },
    Match    { expr: Expr, arms: Vec<(Expr, Vec<Stmt>)>, default: Option<Vec<Stmt>>, line: usize },
    Return   { value: Option<Expr>, line: usize },
    Print    { value: Expr, line: usize },
    ExprStmt { expr: Expr, line: usize },
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    StringLit(String),
    Bool(bool),
    Null,
    Ident    { name: String, line: usize },
    Self_    { line: usize },
    List     { elements: Vec<Expr>, line: usize },
    Dict     { elements: Vec<(Expr, Expr)>, line: usize },
    Lambda   { params: Vec<String>, body: Vec<Stmt>, line: usize },
    Get      { object: Box<Expr>, name: String, line: usize },
    IndexGet { object: Box<Expr>, index: Box<Expr>, line: usize },
    BinOp    { op: BinOpKind, left: Box<Expr>, right: Box<Expr>, line: usize },
    Unary    { op: UnaryOpKind, expr: Box<Expr>, line: usize },
    Call     { callee: Box<Expr>, args: Vec<Expr>, line: usize },
}

#[derive(Debug, Clone)]
pub enum BinOpKind {
    Add, Sub, Mul, Div, Mod,
    Eq, NotEq, Lt, Gt, LtEq, GtEq,
    And, Or,
}

#[derive(Debug, Clone)]
pub enum UnaryOpKind {
    Neg,
    Not,
}
