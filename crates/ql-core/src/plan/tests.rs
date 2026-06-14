use super::plan_select;
use crate::sql::{SelectStatement, parse_query};

#[test]
fn renders_filter_order_and_limit() {
    let statement =
        parse("SELECT name, file FROM functions WHERE complexity > 3 ORDER BY line DESC LIMIT 5");

    let plan = plan_select(&statement).expect("query should plan");

    assert_eq!(
        plan.sql,
        "SELECT name, file FROM functions WHERE (complexity > 3) ORDER BY line DESC LIMIT 5"
    );
}

#[test]
fn renders_join_query() {
    let statement = parse(
        "SELECT functions.name, calls.callee FROM functions JOIN calls ON functions.name = calls.caller",
    );

    let plan = plan_select(&statement).expect("join should plan");

    assert_eq!(
        plan.sql,
        "SELECT functions.name, calls.callee FROM functions JOIN calls ON (functions.name = calls.caller)"
    );
}

#[test]
fn rejects_invalid_identifier() {
    let statement = SelectStatement {
        select: vec![crate::sql::SelectItem::Column("bad-name".to_string())],
        from: crate::sql::TableRef {
            name: "functions".to_string(),
        },
        joins: Vec::new(),
        where_clause: None,
        order_by: Vec::new(),
        limit: None,
    };

    let error = plan_select(&statement).expect_err("bad identifier should fail");

    assert_eq!(error.message, "invalid identifier: bad-name");
}

fn parse(query: &str) -> SelectStatement {
    parse_query(query).expect("query should parse")
}
