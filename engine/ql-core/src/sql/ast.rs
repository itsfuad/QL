#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectStatement {
    pub select: Vec<SelectItem>,
    pub from: TableRef,
    pub joins: Vec<Join>,
    pub where_clause: Option<Expr>,
    pub order_by: Vec<OrderBy>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectItem {
    Wildcard,
    Column(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Join {
    pub kind: JoinKind,
    pub table: TableRef,
    pub on: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderBy {
    pub column: String,
    pub direction: OrderDirection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Identifier(String),
    Literal(Literal),
    Unary {
        operator: UnaryOperator,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        operator: BinaryOperator,
        right: Box<Expr>,
    },
    InList {
        expr: Box<Expr>,
        values: Vec<Expr>,
        negated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnaryOperator {
    Not,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryOperator {
    Eq,
    NotEq,
    Gt,
    Lt,
    Gte,
    Lte,
    And,
    Or,
    Like,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Integer(u64),
    String(String),
}
