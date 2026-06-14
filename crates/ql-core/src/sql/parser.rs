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

        self.matches(TokenMatcher::Semicolon);

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
    Semicolon,
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
        let Some(kind) = token.map(|token| &token.kind) else {
            return false;
        };
        matches!(
            (self, kind),
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
                | (Self::Semicolon, TokenKind::Semicolon)
                | (Self::Star, TokenKind::Star)
                | (Self::Eq, TokenKind::Eq)
                | (Self::NotEq, TokenKind::NotEq)
                | (Self::Gt, TokenKind::Gt)
                | (Self::Lt, TokenKind::Lt)
                | (Self::Gte, TokenKind::Gte)
                | (Self::Lte, TokenKind::Lte)
        )
    }
}

#[cfg(test)]
mod tests;
