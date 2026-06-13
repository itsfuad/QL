use super::ast::{
    BinaryOperator, Expr, Join, JoinKind, Literal, OrderBy, OrderDirection, SelectItem,
    SelectStatement, TableRef, UnaryOperator,
};
use super::lexer::{Token, TokenKind, lex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

pub fn parse_query(input: &str) -> Result<SelectStatement, ParseError> {
    let tokens = lex(input).map_err(|position| ParseError {
        message: "invalid token".to_string(),
        position,
    })?;
    Parser::new(tokens).parse_select()
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }

    fn parse_select(&mut self) -> Result<SelectStatement, ParseError> {
        self.expect_keyword(TokenMatcher::Select, "expected SELECT")?;
        let select = self.parse_select_list()?;
        self.expect_keyword(TokenMatcher::From, "expected FROM")?;
        let from = self.parse_table_ref()?;
        let joins = self.parse_joins()?;
        let where_clause = if self.matches(TokenMatcher::Where) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        let order_by = if self.matches(TokenMatcher::Order) {
            self.expect_keyword(TokenMatcher::By, "expected BY after ORDER")?;
            self.parse_order_by()?
        } else {
            Vec::new()
        };
        let limit = if self.matches(TokenMatcher::Limit) {
            Some(self.parse_limit()?)
        } else {
            None
        };

        if !self.is_done() {
            return Err(self.error_here("unexpected trailing tokens"));
        }

        Ok(SelectStatement {
            select,
            from,
            joins,
            where_clause,
            order_by,
            limit,
        })
    }

    fn parse_select_list(&mut self) -> Result<Vec<SelectItem>, ParseError> {
        let mut items = Vec::new();

        loop {
            if self.matches(TokenMatcher::Star) {
                items.push(SelectItem::Wildcard);
            } else {
                items.push(SelectItem::Column(self.parse_identifier_path()?));
            }

            if !self.matches(TokenMatcher::Comma) {
                break;
            }
        }

        Ok(items)
    }

    fn parse_table_ref(&mut self) -> Result<TableRef, ParseError> {
        Ok(TableRef {
            name: self.parse_identifier_path()?,
        })
    }

    fn parse_joins(&mut self) -> Result<Vec<Join>, ParseError> {
        let mut joins = Vec::new();

        while self.matches(TokenMatcher::Join) {
            let table = self.parse_table_ref()?;
            self.expect_keyword(TokenMatcher::On, "expected ON after JOIN table")?;
            let on = self.parse_expression()?;
            joins.push(Join {
                kind: JoinKind::Inner,
                table,
                on,
            });
        }

        Ok(joins)
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderBy>, ParseError> {
        let mut clauses = Vec::new();

        loop {
            let column = self.parse_identifier_path()?;
            let direction = if self.matches(TokenMatcher::Desc) {
                OrderDirection::Desc
            } else {
                self.matches(TokenMatcher::Asc);
                OrderDirection::Asc
            };

            clauses.push(OrderBy { column, direction });
            if !self.matches(TokenMatcher::Comma) {
                break;
            }
        }

        Ok(clauses)
    }

    fn parse_limit(&mut self) -> Result<u64, ParseError> {
        match self.advance() {
            Some(Token {
                kind: TokenKind::Integer(value),
                ..
            }) => Ok(*value),
            _ => Err(self.error_here("expected integer after LIMIT")),
        }
    }

    fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_and()?;

        while self.matches(TokenMatcher::Or) {
            expr = Expr::Binary {
                left: Box::new(expr),
                operator: BinaryOperator::Or,
                right: Box::new(self.parse_and()?),
            };
        }

        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_not()?;

        while self.matches(TokenMatcher::And) {
            expr = Expr::Binary {
                left: Box::new(expr),
                operator: BinaryOperator::And,
                right: Box::new(self.parse_not()?),
            };
        }

        Ok(expr)
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if self.matches(TokenMatcher::Not) {
            return Ok(Expr::Unary {
                operator: UnaryOperator::Not,
                expr: Box::new(self.parse_not()?),
            });
        }

        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_primary()?;

        if self.matches(TokenMatcher::Not) {
            if self.matches(TokenMatcher::In) {
                return self.parse_in_list(left, true);
            }
            return Err(self.error_here("expected IN after NOT"));
        }

        if self.matches(TokenMatcher::In) {
            return self.parse_in_list(left, false);
        }

        if self.matches(TokenMatcher::Like) {
            return Ok(Expr::Binary {
                left: Box::new(left),
                operator: BinaryOperator::Like,
                right: Box::new(self.parse_primary()?),
            });
        }

        let operator = if self.matches(TokenMatcher::Eq) {
            Some(BinaryOperator::Eq)
        } else if self.matches(TokenMatcher::NotEq) {
            Some(BinaryOperator::NotEq)
        } else if self.matches(TokenMatcher::Gte) {
            Some(BinaryOperator::Gte)
        } else if self.matches(TokenMatcher::Lte) {
            Some(BinaryOperator::Lte)
        } else if self.matches(TokenMatcher::Gt) {
            Some(BinaryOperator::Gt)
        } else if self.matches(TokenMatcher::Lt) {
            Some(BinaryOperator::Lt)
        } else {
            None
        };

        match operator {
            Some(operator) => Ok(Expr::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(self.parse_primary()?),
            }),
            None => Ok(left),
        }
    }

    fn parse_in_list(&mut self, left: Expr, negated: bool) -> Result<Expr, ParseError> {
        self.expect_keyword(TokenMatcher::LParen, "expected ( after IN")?;
        let mut values = Vec::new();

        loop {
            values.push(self.parse_primary()?);
            if !self.matches(TokenMatcher::Comma) {
                break;
            }
        }

        self.expect_keyword(TokenMatcher::RParen, "expected ) after IN list")?;

        Ok(Expr::InList {
            expr: Box::new(left),
            values,
            negated,
        })
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        if self.matches(TokenMatcher::LParen) {
            let expr = self.parse_expression()?;
            self.expect_keyword(TokenMatcher::RParen, "expected )")?;
            return Ok(expr);
        }

        match self.advance() {
            Some(Token {
                kind: TokenKind::Identifier(_),
                ..
            }) => {
                self.index -= 1;
                Ok(Expr::Identifier(self.parse_identifier_path()?))
            }
            Some(Token {
                kind: TokenKind::Integer(value),
                ..
            }) => Ok(Expr::Literal(Literal::Integer(*value))),
            Some(Token {
                kind: TokenKind::String(value),
                ..
            }) => Ok(Expr::Literal(Literal::String(value.clone()))),
            _ => Err(self.error_here("expected identifier or literal")),
        }
    }

    fn parse_identifier_path(&mut self) -> Result<String, ParseError> {
        let mut value = match self.advance() {
            Some(Token {
                kind: TokenKind::Identifier(value),
                ..
            }) => value.clone(),
            _ => return Err(self.error_here("expected identifier")),
        };

        while self.matches(TokenMatcher::Dot) {
            value.push('.');
            match self.advance() {
                Some(Token {
                    kind: TokenKind::Identifier(next),
                    ..
                }) => value.push_str(next),
                _ => return Err(self.error_here("expected identifier after .")),
            }
        }

        Ok(value)
    }

    fn expect_keyword(
        &mut self,
        matcher: TokenMatcher,
        message: &'static str,
    ) -> Result<(), ParseError> {
        if self.matches(matcher) {
            Ok(())
        } else {
            Err(self.error_here(message))
        }
    }

    fn matches(&mut self, matcher: TokenMatcher) -> bool {
        if matcher.matches(self.peek()) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn advance(&mut self) -> Option<&Token> {
        let token = self.tokens.get(self.index)?;
        self.index += 1;
        Some(token)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.index)
    }

    fn is_done(&self) -> bool {
        self.index >= self.tokens.len()
    }

    fn error_here(&self, message: &str) -> ParseError {
        ParseError {
            message: message.to_string(),
            position: self.peek().map_or(0, |token| token.start),
        }
    }
}

#[derive(Clone, Copy)]
enum TokenMatcher {
    Select,
    From,
    Join,
    On,
    Where,
    Order,
    By,
    Limit,
    Asc,
    Desc,
    And,
    Or,
    Not,
    In,
    Like,
    Comma,
    Dot,
    LParen,
    RParen,
    Star,
    Eq,
    NotEq,
    Gt,
    Lt,
    Gte,
    Lte,
}

impl TokenMatcher {
    fn matches(self, token: Option<&Token>) -> bool {
        matches!(
            token.map(|token| &token.kind),
            Some(kind) if match (self, kind) {
                (Self::Select, TokenKind::Select)
                | (Self::From, TokenKind::From)
                | (Self::Join, TokenKind::Join)
                | (Self::On, TokenKind::On)
                | (Self::Where, TokenKind::Where)
                | (Self::Order, TokenKind::Order)
                | (Self::By, TokenKind::By)
                | (Self::Limit, TokenKind::Limit)
                | (Self::Asc, TokenKind::Asc)
                | (Self::Desc, TokenKind::Desc)
                | (Self::And, TokenKind::And)
                | (Self::Or, TokenKind::Or)
                | (Self::Not, TokenKind::Not)
                | (Self::In, TokenKind::In)
                | (Self::Like, TokenKind::Like)
                | (Self::Comma, TokenKind::Comma)
                | (Self::Dot, TokenKind::Dot)
                | (Self::LParen, TokenKind::LParen)
                | (Self::RParen, TokenKind::RParen)
                | (Self::Star, TokenKind::Star)
                | (Self::Eq, TokenKind::Eq)
                | (Self::NotEq, TokenKind::NotEq)
                | (Self::Gt, TokenKind::Gt)
                | (Self::Lt, TokenKind::Lt)
                | (Self::Gte, TokenKind::Gte)
                | (Self::Lte, TokenKind::Lte) => true,
                _ => false,
            }
        )
    }
}

#[cfg(test)]
mod tests {
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
        let query =
            parse_query("SELECT name, file, line FROM functions").expect("query should parse");

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
        let query =
            parse_query("SELECT name FROM functions WHERE visibility IN ('public', 'private')")
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
        let query = parse_query("SELECT name FROM functions WHERE file NOT IN ('a.go', 'b.go')")
            .expect("query should parse");

        assert_eq!(
            query.where_clause,
            Some(Expr::InList {
                expr: Box::new(Expr::Identifier("file".to_string())),
                values: vec![
                    Expr::Literal(Literal::String("a.go".to_string())),
                    Expr::Literal(Literal::String("b.go".to_string())),
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
        let error =
            parse_query("SELECT name FROM functions WHERE @").expect_err("query should fail");

        assert_eq!(
            error,
            ParseError {
                message: "invalid token".to_string(),
                position: 33,
            }
        );
    }
}
