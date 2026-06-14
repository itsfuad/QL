use super::super::ast::{
    BinaryOperator, Expr, Join, JoinKind, Literal, OrderBy, OrderDirection, SelectItem,
    SelectStatement, TableRef, UnaryOperator,
};
use super::{ParseError, parse_query};

#[test]
fn parses_select_wildcard() {
    let query = parse_query("SELECT * FROM functions").expect("query should parse");

    assert_eq!(
        query,
        SelectStatement {
            select: vec![SelectItem::Wildcard],
            from: TableRef {
                name: "functions".to_string(),
            },
            joins: vec![],
            where_clause: None,
            order_by: vec![],
            limit: None,
        }
    );
}

#[test]
fn parses_column_list() {
    let query = parse_query("SELECT name, file, line FROM functions").expect("query should parse");

    assert_eq!(
        query.select,
        vec![
            SelectItem::Column("name".to_string()),
            SelectItem::Column("file".to_string()),
            SelectItem::Column("line".to_string()),
        ]
    );
}

#[test]
fn parses_where_comparison() {
    let query = parse_query("SELECT name FROM functions WHERE complexity >= 10")
        .expect("query should parse");

    assert_eq!(
        query.where_clause,
        Some(Expr::Binary {
            left: Box::new(Expr::Identifier("complexity".to_string())),
            operator: BinaryOperator::Gte,
            right: Box::new(Expr::Literal(Literal::Integer(10))),
        })
    );
}

#[test]
fn parses_boolean_precedence() {
    let query = parse_query(
        "SELECT name FROM functions WHERE has_test = 0 OR complexity > 8 AND line < 20",
    )
    .expect("query should parse");

    assert_eq!(
        query.where_clause,
        Some(Expr::Binary {
            left: Box::new(Expr::Binary {
                left: Box::new(Expr::Identifier("has_test".to_string())),
                operator: BinaryOperator::Eq,
                right: Box::new(Expr::Literal(Literal::Integer(0))),
            }),
            operator: BinaryOperator::Or,
            right: Box::new(Expr::Binary {
                left: Box::new(Expr::Binary {
                    left: Box::new(Expr::Identifier("complexity".to_string())),
                    operator: BinaryOperator::Gt,
                    right: Box::new(Expr::Literal(Literal::Integer(8))),
                }),
                operator: BinaryOperator::And,
                right: Box::new(Expr::Binary {
                    left: Box::new(Expr::Identifier("line".to_string())),
                    operator: BinaryOperator::Lt,
                    right: Box::new(Expr::Literal(Literal::Integer(20))),
                }),
            }),
        })
    );
}

#[test]
fn parses_not_expression() {
    let query = parse_query("SELECT name FROM functions WHERE NOT has_test = 1")
        .expect("query should parse");

    assert_eq!(
        query.where_clause,
        Some(Expr::Unary {
            operator: UnaryOperator::Not,
            expr: Box::new(Expr::Binary {
                left: Box::new(Expr::Identifier("has_test".to_string())),
                operator: BinaryOperator::Eq,
                right: Box::new(Expr::Literal(Literal::Integer(1))),
            }),
        })
    );
}

#[test]
fn parses_in_list() {
    let query = parse_query("SELECT name FROM functions WHERE visibility IN ('public', 'private')")
        .expect("query should parse");

    assert_eq!(
        query.where_clause,
        Some(Expr::InList {
            expr: Box::new(Expr::Identifier("visibility".to_string())),
            values: vec![
                Expr::Literal(Literal::String("public".to_string())),
                Expr::Literal(Literal::String("private".to_string())),
            ],
            negated: false,
        })
    );
}

#[test]
fn parses_not_in_list() {
    let query = parse_query("SELECT name FROM functions WHERE file NOT IN ('a.rs', 'b.rs')")
        .expect("query should parse");

    assert_eq!(
        query.where_clause,
        Some(Expr::InList {
            expr: Box::new(Expr::Identifier("file".to_string())),
            values: vec![
                Expr::Literal(Literal::String("a.rs".to_string())),
                Expr::Literal(Literal::String("b.rs".to_string())),
            ],
            negated: true,
        })
    );
}

#[test]
fn parses_like_operator() {
    let query = parse_query("SELECT name FROM functions WHERE file LIKE '%_test%'")
        .expect("query should parse");

    assert_eq!(
        query.where_clause,
        Some(Expr::Binary {
            left: Box::new(Expr::Identifier("file".to_string())),
            operator: BinaryOperator::Like,
            right: Box::new(Expr::Literal(Literal::String("%_test%".to_string()))),
        })
    );
}

#[test]
fn parses_order_by_and_limit() {
    let query =
        parse_query("SELECT name FROM functions ORDER BY complexity DESC, line ASC LIMIT 20")
            .expect("query should parse");

    assert_eq!(
        query.order_by,
        vec![
            OrderBy {
                column: "complexity".to_string(),
                direction: OrderDirection::Desc,
            },
            OrderBy {
                column: "line".to_string(),
                direction: OrderDirection::Asc,
            },
        ]
    );
    assert_eq!(query.limit, Some(20));
}

#[test]
fn parses_trailing_semicolon() {
    let query = parse_query("SELECT name FROM functions;").expect("query should parse");

    assert_eq!(query.select, vec![SelectItem::Column("name".to_string())]);
}

#[test]
fn parses_join() {
    let query = parse_query(
        "SELECT functions.name FROM functions JOIN calls ON functions.name = calls.caller",
    )
    .expect("query should parse");

    assert_eq!(
        query.joins,
        vec![Join {
            kind: JoinKind::Inner,
            table: TableRef {
                name: "calls".to_string(),
            },
            on: Expr::Binary {
                left: Box::new(Expr::Identifier("functions.name".to_string())),
                operator: BinaryOperator::Eq,
                right: Box::new(Expr::Identifier("calls.caller".to_string())),
            },
        }]
    );
}

#[test]
fn reports_missing_from() {
    let error = parse_query("SELECT name functions").expect_err("query should fail");

    assert_eq!(
        error,
        ParseError {
            message: "expected FROM".to_string(),
            position: 12,
        }
    );
}

#[test]
fn reports_invalid_token_position() {
    let error = parse_query("SELECT name FROM functions WHERE @").expect_err("query should fail");

    assert_eq!(
        error,
        ParseError {
            message: "invalid token".to_string(),
            position: 33,
        }
    );
}
