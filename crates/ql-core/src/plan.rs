use std::fmt;

use crate::sql::{
    BinaryOperator, Expr, Join, JoinKind, Literal, OrderBy, OrderDirection, SelectItem,
    SelectStatement, UnaryOperator,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedQuery {
    pub sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError {
    pub message: String,
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub fn plan_select(statement: &SelectStatement) -> Result<PlannedQuery, PlanError> {
    Ok(PlannedQuery {
        sql: render_select(statement)?,
    })
}

fn render_select(statement: &SelectStatement) -> Result<String, PlanError> {
    let mut sql = String::from("SELECT ");
    sql.push_str(&render_select_list(&statement.select)?);
    sql.push_str(" FROM ");
    sql.push_str(&render_identifier(&statement.from.name)?);

    for join in &statement.joins {
        sql.push(' ');
        sql.push_str(&render_join(join)?);
    }

    if let Some(where_clause) = &statement.where_clause {
        sql.push_str(" WHERE ");
        sql.push_str(&render_expr(where_clause)?);
    }

    if !statement.order_by.is_empty() {
        sql.push_str(" ORDER BY ");
        sql.push_str(&render_order_by(&statement.order_by)?);
    }

    if let Some(limit) = statement.limit {
        sql.push_str(" LIMIT ");
        sql.push_str(&limit.to_string());
    }

    Ok(sql)
}

fn render_select_list(items: &[SelectItem]) -> Result<String, PlanError> {
    let mut rendered = Vec::with_capacity(items.len());

    for item in items {
        rendered.push(match item {
            SelectItem::Wildcard => "*".to_string(),
            SelectItem::Column(column) => render_identifier(column)?,
        });
    }

    Ok(rendered.join(", "))
}

fn render_join(join: &Join) -> Result<String, PlanError> {
    let kind = match join.kind {
        JoinKind::Inner => "JOIN",
    };

    Ok(format!(
        "{kind} {} ON {}",
        render_identifier(&join.table.name)?,
        render_expr(&join.on)?,
    ))
}

fn render_order_by(clauses: &[OrderBy]) -> Result<String, PlanError> {
    let mut rendered = Vec::with_capacity(clauses.len());

    for clause in clauses {
        let direction = match clause.direction {
            OrderDirection::Asc => "ASC",
            OrderDirection::Desc => "DESC",
        };
        rendered.push(format!(
            "{} {}",
            render_identifier(&clause.column)?,
            direction
        ));
    }

    Ok(rendered.join(", "))
}

fn render_expr(expr: &Expr) -> Result<String, PlanError> {
    match expr {
        Expr::Identifier(identifier) => render_identifier(identifier),
        Expr::Literal(literal) => Ok(render_literal(literal)),
        Expr::Unary { operator, expr } => {
            let operator = match operator {
                UnaryOperator::Not => "NOT",
            };
            Ok(format!("{operator} ({})", render_expr(expr)?))
        }
        Expr::Binary {
            left,
            operator,
            right,
        } => Ok(format!(
            "({} {} {})",
            render_expr(left)?,
            render_binary_operator(operator),
            render_expr(right)?,
        )),
        Expr::InList {
            expr,
            values,
            negated,
        } => {
            if values.is_empty() {
                return Err(PlanError {
                    message: "empty IN lists are not supported".to_string(),
                });
            }

            let mut rendered_values = Vec::with_capacity(values.len());
            for value in values {
                rendered_values.push(render_expr(value)?);
            }

            let negated = if *negated { " NOT" } else { "" };
            Ok(format!(
                "({}{negated} IN ({}))",
                render_expr(expr)?,
                rendered_values.join(", "),
            ))
        }
    }
}

fn render_binary_operator(operator: &BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::Eq => "=",
        BinaryOperator::NotEq => "!=",
        BinaryOperator::Gt => ">",
        BinaryOperator::Lt => "<",
        BinaryOperator::Gte => ">=",
        BinaryOperator::Lte => "<=",
        BinaryOperator::And => "AND",
        BinaryOperator::Or => "OR",
        BinaryOperator::Like => "LIKE",
    }
}

fn render_literal(literal: &Literal) -> String {
    match literal {
        Literal::Integer(value) => value.to_string(),
        Literal::String(value) => format!("'{}'", value.replace('\'', "''")),
    }
}

fn render_identifier(identifier: &str) -> Result<String, PlanError> {
    let mut segments = Vec::new();

    for segment in identifier.split('.') {
        if !is_valid_identifier(segment) {
            return Err(PlanError {
                message: format!("invalid identifier: {identifier}"),
            });
        }
        segments.push(segment);
    }

    Ok(segments.join("."))
}

fn is_valid_identifier(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }

    chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
}

#[cfg(test)]
mod tests {
    use super::plan_select;
    use crate::sql::{SelectStatement, parse_query};

    #[test]
    fn renders_filter_order_and_limit() {
        let statement = parse(
            "SELECT name, file FROM functions WHERE complexity > 3 ORDER BY line DESC LIMIT 5",
        );

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
}
