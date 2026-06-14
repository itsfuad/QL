use super::ast::{
    BinaryOperator, Expr, Join, Literal, OrderBy, OrderDirection, SelectItem, SelectStatement,
    TableRef, UnaryOperator,
};
use super::diagnostic::{Diagnostic, Label, Severity, SourceFile, Span};
use super::lexer::{Token, TokenKind, lex};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_query(input: &str) -> Result<SelectStatement, ParseError> {
    let tokens = lex(input).map_err(|position| ParseError::invalid_token(position))?;
    Parser::new(tokens).parse_select()
}

impl ParseError {
    fn invalid_token(position: usize) -> Self {
        Self::new("E000", "invalid token", Span::point(0, position))
    }

    fn new(code: &str, message: &str, span: Span) -> Self {
        Self::single(Diagnostic {
            severity: Severity::Error,
            code: Some(code.to_string()),
            message: message.to_string(),
            labels: vec![Label {
                span,
                message: String::new(),
            }],
            notes: Vec::new(),
        })
    }

    fn at(message: &str, span: Span) -> Self {
        Self::new("E001", message, span)
    }

    fn single(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }

    fn from_diagnostics(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }

    pub fn message(&self) -> &str {
        self.diagnostics
            .first()
            .map_or("parse error", |diagnostic| diagnostic.message.as_str())
    }

    pub fn position(&self) -> usize {
        self.diagnostics
            .first()
            .and_then(|diagnostic| diagnostic.labels.first())
            .map_or(0, |label| label.span.start)
    }

    pub fn render(&self, file_name: &str, input: &str) -> String {
        let file = SourceFile::new(file_name, input);
        self.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.render(&[file.clone()]))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    eof: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        let eof = tokens.last().map_or(0, |token| token.end);
        Self {
            tokens,
            index: 0,
            eof,
            diagnostics: Vec::new(),
        }
    }

    fn parse_select(&mut self) -> Result<SelectStatement, ParseError> {
        if !self.matches_kind(|kind| matches!(kind, TokenKind::Select)) {
            self.record_error(self.error_here("expected SELECT"));
            self.recover_to_clause_boundary();
        }
        let distinct = self.matches_kind(|kind| matches!(kind, TokenKind::Distinct));
        let select = self.parse_or_recover(Vec::new(), Parser::parse_select_list);
        if !self.matches_kind(|kind| matches!(kind, TokenKind::From)) {
            self.record_error(self.error_here("expected FROM"));
            self.recover_to_clause_boundary();
            self.matches_kind(|kind| matches!(kind, TokenKind::From));
        }
        let from = self.parse_or_recover(
            TableRef {
                name: String::new(),
                alias: None,
            },
            Parser::parse_table_ref,
        );
        let joins = self.parse_joins();
        let where_clause = if self.matches_kind(|kind| matches!(kind, TokenKind::Where)) {
            self.parse_or_recover(None, |parser| parser.parse_expression().map(Some))
        } else {
            None
        };
        let group_by = if self.matches_kind(|kind| matches!(kind, TokenKind::Group)) {
            if !self.matches_kind(|kind| matches!(kind, TokenKind::By)) {
                self.record_error(self.error_here("expected BY after GROUP"));
                self.recover_to_clause_boundary();
                self.matches_kind(|kind| matches!(kind, TokenKind::By));
            }
            self.parse_or_recover(Vec::new(), Parser::parse_group_by)
        } else {
            Vec::new()
        };
        let having = if self.matches_kind(|kind| matches!(kind, TokenKind::Having)) {
            self.parse_or_recover(None, |parser| parser.parse_expression().map(Some))
        } else {
            None
        };
        let order_by = if self.matches_kind(|kind| matches!(kind, TokenKind::Order)) {
            if !self.matches_kind(|kind| matches!(kind, TokenKind::By)) {
                self.record_error(self.error_here("expected BY after ORDER"));
                self.recover_to_clause_boundary();
                self.matches_kind(|kind| matches!(kind, TokenKind::By));
            }
            self.parse_or_recover(Vec::new(), Parser::parse_order_by)
        } else {
            Vec::new()
        };
        let limit = if self.matches_kind(|kind| matches!(kind, TokenKind::Limit)) {
            self.parse_or_recover(None, |parser| parser.parse_limit().map(Some))
        } else {
            None
        };

        self.matches_kind(|kind| matches!(kind, TokenKind::Semicolon));

        if !self.is_done() {
            self.record_error(self.error_here("unexpected trailing tokens"));
            self.recover_to_clause_boundary();
        }

        if self.diagnostics.is_empty() {
            Ok(SelectStatement {
                select,
                distinct,
                from,
                joins,
                where_clause,
                group_by,
                having,
                order_by,
                limit,
            })
        } else {
            Err(ParseError::from_diagnostics(std::mem::take(
                &mut self.diagnostics,
            )))
        }
    }

    fn parse_or_recover<T>(
        &mut self,
        fallback: T,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> T {
        match parse(self) {
            Ok(value) => value,
            Err(error) => {
                self.record_error(error);
                self.recover_to_clause_boundary();
                fallback
            }
        }
    }

    fn record_error(&mut self, error: ParseError) {
        self.diagnostics.extend(error.diagnostics);
    }

    fn recover_to_clause_boundary(&mut self) {
        while let Some(token) = self.peek() {
            if matches!(
                token.kind,
                TokenKind::From
                    | TokenKind::Group
                    | TokenKind::Join
                    | TokenKind::Where
                    | TokenKind::Having
                    | TokenKind::Order
                    | TokenKind::Limit
                    | TokenKind::Semicolon
            ) {
                break;
            }
            self.index += 1;
        }
    }

    fn parse_joins(&mut self) -> Vec<Join> {
        let mut joins = Vec::new();

        while self.matches_kind(|kind| matches!(kind, TokenKind::Join)) {
            match self.parse_join() {
                Ok(join) => joins.push(join),
                Err(error) => {
                    self.record_error(error);
                    self.recover_to_clause_boundary();
                }
            }
        }

        joins
    }

    fn parse_join(&mut self) -> Result<Join, ParseError> {
        let table = self.parse_table_ref()?;
        self.expect_kind(
            |kind| matches!(kind, TokenKind::On),
            "expected ON after JOIN table",
        )?;
        let on = self.parse_expression()?;
        Ok(Join { table, on })
    }

    fn parse_select_list(&mut self) -> Result<Vec<SelectItem>, ParseError> {
        let mut items = Vec::new();

        loop {
            items.push(self.parse_select_item()?);

            if !self.matches_kind(|kind| matches!(kind, TokenKind::Comma)) {
                break;
            }
        }

        Ok(items)
    }

    fn parse_select_item(&mut self) -> Result<SelectItem, ParseError> {
        if self.matches_kind(|kind| matches!(kind, TokenKind::Star)) {
            return Ok(SelectItem::Wildcard);
        }

        let name = self.parse_identifier_path()?;
        let alias = self.parse_alias()?;
        Ok(SelectItem::Column { name, alias })
    }

    fn parse_table_ref(&mut self) -> Result<TableRef, ParseError> {
        Ok(TableRef {
            name: self.parse_identifier_path()?,
            alias: self.parse_alias()?,
        })
    }

    fn parse_order_by(&mut self) -> Result<Vec<OrderBy>, ParseError> {
        let mut clauses = Vec::new();

        loop {
            let column = self.parse_identifier_path()?;
            let direction = if self.matches_kind(|kind| matches!(kind, TokenKind::Desc)) {
                OrderDirection::Desc
            } else {
                self.matches_kind(|kind| matches!(kind, TokenKind::Asc));
                OrderDirection::Asc
            };

            clauses.push(OrderBy { column, direction });
            if !self.matches_kind(|kind| matches!(kind, TokenKind::Comma)) {
                break;
            }
        }

        Ok(clauses)
    }

    fn parse_group_by(&mut self) -> Result<Vec<String>, ParseError> {
        let mut columns = Vec::new();

        loop {
            columns.push(self.parse_identifier_path()?);
            if !self.matches_kind(|kind| matches!(kind, TokenKind::Comma)) {
                break;
            }
        }

        Ok(columns)
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
        self.parse_expression_bp(0)
    }

    fn parse_expression_bp(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut left = self.nud()?;

        loop {
            let Some(op) = self.peek_infix_op() else {
                break;
            };
            let (left_bp, right_bp) = op.binding_power();
            if left_bp < min_bp {
                break;
            }

            self.advance();
            left = self.led(left, op, right_bp)?;
        }

        Ok(left)
    }

    fn nud(&mut self) -> Result<Expr, ParseError> {
        let token = self
            .advance()
            .cloned()
            .ok_or_else(|| self.error_here("expected expression"))?;

        match token.kind {
            TokenKind::LParen => {
                let expr = self.parse_expression_bp(0)?;
                self.expect_kind(|kind| matches!(kind, TokenKind::RParen), "expected )")?;
                Ok(expr)
            }
            TokenKind::Not => Ok(Expr::Unary {
                operator: UnaryOperator::Not,
                expr: Box::new(self.parse_expression_bp(PREFIX_NOT_BP)?),
            }),
            TokenKind::Identifier(mut value) => {
                while self.matches_kind(|kind| matches!(kind, TokenKind::Dot)) {
                    value.push('.');
                    match self.advance() {
                        Some(Token {
                            kind: TokenKind::Identifier(next),
                            ..
                        }) => value.push_str(next),
                        _ => return Err(self.error_here("expected identifier after .")),
                    }
                }

                Ok(Expr::Identifier(value))
            }
            TokenKind::Integer(value) => Ok(Expr::Literal(Literal::Integer(value))),
            TokenKind::String(value) => Ok(Expr::Literal(Literal::String(value))),
            _ => Err(self.error_here("expected identifier or literal")),
        }
    }

    fn led(&mut self, left: Expr, op: InfixOp, right_bp: u8) -> Result<Expr, ParseError> {
        match op {
            InfixOp::Or => self.led_binary(left, BinaryOperator::Or, right_bp),
            InfixOp::And => self.led_binary(left, BinaryOperator::And, right_bp),
            InfixOp::Eq => self.led_binary(left, BinaryOperator::Eq, right_bp),
            InfixOp::NotEq => self.led_binary(left, BinaryOperator::NotEq, right_bp),
            InfixOp::Gt => self.led_binary(left, BinaryOperator::Gt, right_bp),
            InfixOp::Lt => self.led_binary(left, BinaryOperator::Lt, right_bp),
            InfixOp::Gte => self.led_binary(left, BinaryOperator::Gte, right_bp),
            InfixOp::Lte => self.led_binary(left, BinaryOperator::Lte, right_bp),
            InfixOp::Like => self.led_binary(left, BinaryOperator::Like, right_bp),
            InfixOp::In => self.led_in_list(left, false),
            InfixOp::NotIn => {
                self.expect_kind(
                    |kind| matches!(kind, TokenKind::In),
                    "expected IN after NOT",
                )?;
                self.led_in_list(left, true)
            }
        }
    }

    fn led_binary(
        &mut self,
        left: Expr,
        operator: BinaryOperator,
        right_bp: u8,
    ) -> Result<Expr, ParseError> {
        let right = self.parse_expression_bp(right_bp)?;
        Ok(Expr::Binary {
            left: Box::new(left),
            operator,
            right: Box::new(right),
        })
    }

    fn led_in_list(&mut self, left: Expr, negated: bool) -> Result<Expr, ParseError> {
        self.expect_kind(
            |kind| matches!(kind, TokenKind::LParen),
            "expected ( after IN",
        )?;
        let mut values = Vec::new();

        loop {
            values.push(self.parse_expression_bp(0)?);
            if !self.matches_kind(|kind| matches!(kind, TokenKind::Comma)) {
                break;
            }
        }

        self.expect_kind(
            |kind| matches!(kind, TokenKind::RParen),
            "expected ) after IN list",
        )?;

        Ok(Expr::InList {
            expr: Box::new(left),
            values,
            negated,
        })
    }

    fn parse_identifier_path(&mut self) -> Result<String, ParseError> {
        let mut value = self.parse_identifier()?;

        while self.matches_kind(|kind| matches!(kind, TokenKind::Dot)) {
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

    fn parse_alias(&mut self) -> Result<Option<String>, ParseError> {
        if !self.matches_kind(|kind| matches!(kind, TokenKind::As)) {
            return Ok(None);
        }

        Ok(Some(self.parse_identifier()?))
    }

    fn parse_identifier(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Some(Token {
                kind: TokenKind::Identifier(value),
                ..
            }) => Ok(value.clone()),
            _ => Err(self.error_here("expected identifier")),
        }
    }

    fn expect_kind<F>(&mut self, predicate: F, message: &'static str) -> Result<(), ParseError>
    where
        F: FnOnce(&TokenKind) -> bool,
    {
        if self.matches_kind(predicate) {
            Ok(())
        } else {
            Err(self.error_here(message))
        }
    }

    fn matches_kind<F>(&mut self, predicate: F) -> bool
    where
        F: FnOnce(&TokenKind) -> bool,
    {
        if self.peek().is_some_and(|token| predicate(&token.kind)) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn peek_next_matches_kind<F>(&self, predicate: F) -> bool
    where
        F: FnOnce(&TokenKind) -> bool,
    {
        self.tokens
            .get(self.index + 1)
            .is_some_and(|token| predicate(&token.kind))
    }

    fn peek_infix_op(&self) -> Option<InfixOp> {
        match self.peek().map(|token| &token.kind) {
            Some(TokenKind::Or) => Some(InfixOp::Or),
            Some(TokenKind::And) => Some(InfixOp::And),
            Some(TokenKind::Eq) => Some(InfixOp::Eq),
            Some(TokenKind::NotEq) => Some(InfixOp::NotEq),
            Some(TokenKind::Gt) => Some(InfixOp::Gt),
            Some(TokenKind::Lt) => Some(InfixOp::Lt),
            Some(TokenKind::Gte) => Some(InfixOp::Gte),
            Some(TokenKind::Lte) => Some(InfixOp::Lte),
            Some(TokenKind::Like) => Some(InfixOp::Like),
            Some(TokenKind::In) => Some(InfixOp::In),
            Some(TokenKind::Not)
                if self.peek_next_matches_kind(|kind| matches!(kind, TokenKind::In)) =>
            {
                Some(InfixOp::NotIn)
            }
            _ => None,
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
        let span = self.peek().map_or(Span::point(0, self.eof), |token| {
            Span::new(0, token.start, token.end)
        });
        ParseError::at(message, span)
    }
}

const PREFIX_NOT_BP: u8 = 29;
// One step below comparison operators, so `NOT active = true` parses as
// `NOT (active = true)` while `NOT active AND ...` still stops before `AND`.

#[derive(Clone, Copy)]
enum InfixOp {
    Or,
    And,
    Eq,
    NotEq,
    Gt,
    Lt,
    Gte,
    Lte,
    Like,
    In,
    NotIn,
}

impl InfixOp {
    fn binding_power(self) -> (u8, u8) {
        match self {
            Self::Or => (10, 11),
            Self::And => (20, 21),
            Self::Eq
            | Self::NotEq
            | Self::Gt
            | Self::Lt
            | Self::Gte
            | Self::Lte
            | Self::Like
            | Self::In
            | Self::NotIn => (30, 31),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ast::{
        BinaryOperator, Expr, Join, Literal, OrderBy, OrderDirection, SelectItem, SelectStatement,
        TableRef, UnaryOperator,
    };
    use super::parse_query;

    #[test]
    fn parses_select_wildcard() {
        let query = parse_query("SELECT * FROM functions").expect("query should parse");

        assert_eq!(
            query,
            SelectStatement {
                select: vec![SelectItem::Wildcard],
                distinct: false,
                from: TableRef {
                    name: "functions".to_string(),
                    alias: None,
                },
                joins: vec![],
                where_clause: None,
                group_by: vec![],
                having: None,
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
                SelectItem::Column {
                    name: "name".to_string(),
                    alias: None,
                },
                SelectItem::Column {
                    name: "file".to_string(),
                    alias: None,
                },
                SelectItem::Column {
                    name: "line".to_string(),
                    alias: None,
                },
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
    fn parses_not_with_and_precedence() {
        let query = parse_query("SELECT name FROM functions WHERE NOT has_test = 1 AND line < 20")
            .expect("query should parse");

        assert_eq!(
            query.where_clause,
            Some(Expr::Binary {
                left: Box::new(Expr::Unary {
                    operator: UnaryOperator::Not,
                    expr: Box::new(Expr::Binary {
                        left: Box::new(Expr::Identifier("has_test".to_string())),
                        operator: BinaryOperator::Eq,
                        right: Box::new(Expr::Literal(Literal::Integer(1))),
                    }),
                }),
                operator: BinaryOperator::And,
                right: Box::new(Expr::Binary {
                    left: Box::new(Expr::Identifier("line".to_string())),
                    operator: BinaryOperator::Lt,
                    right: Box::new(Expr::Literal(Literal::Integer(20))),
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

        assert_eq!(
            query.select,
            vec![SelectItem::Column {
                name: "name".to_string(),
                alias: None,
            }]
        );
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
                table: TableRef {
                    name: "calls".to_string(),
                    alias: None,
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
    fn parses_join_aliases() {
        let query =
            parse_query("SELECT f.name FROM functions AS f JOIN calls AS c ON f.name = c.caller")
                .expect("query should parse");

        assert_eq!(
            query.from,
            TableRef {
                name: "functions".to_string(),
                alias: Some("f".to_string()),
            }
        );
        assert_eq!(
            query.joins,
            vec![Join {
                table: TableRef {
                    name: "calls".to_string(),
                    alias: Some("c".to_string()),
                },
                on: Expr::Binary {
                    left: Box::new(Expr::Identifier("f.name".to_string())),
                    operator: BinaryOperator::Eq,
                    right: Box::new(Expr::Identifier("c.caller".to_string())),
                },
            }]
        );
    }

    #[test]
    fn reports_missing_from() {
        let error = parse_query("SELECT name functions").expect_err("query should fail");

        assert_eq!(error.message(), "expected FROM");
        assert_eq!(error.position(), 12);
    }

    #[test]
    fn reports_invalid_token_position() {
        let error =
            parse_query("SELECT name FROM functions WHERE @").expect_err("query should fail");

        assert_eq!(error.message(), "invalid token");
        assert_eq!(error.position(), 33);
    }

    #[test]
    fn recovers_and_reports_multiple_errors() {
        let error = parse_query("SELECT name file FROM functions WHERE complexity >")
            .expect_err("query should fail");

        assert_eq!(error.diagnostics.len(), 2);
        assert_eq!(error.diagnostics[0].message, "expected FROM");
        assert_eq!(error.diagnostics[1].message, "expected expression");
    }

    #[test]
    fn parses_distinct_aliases() {
        let query = parse_query("SELECT DISTINCT name AS n FROM functions AS f")
            .expect("query should parse");

        assert!(query.distinct);
        assert_eq!(
            query.select,
            vec![SelectItem::Column {
                name: "name".to_string(),
                alias: Some("n".to_string()),
            }]
        );
        assert_eq!(
            query.from,
            TableRef {
                name: "functions".to_string(),
                alias: Some("f".to_string()),
            }
        );
    }

    #[test]
    fn parses_group_by_and_having() {
        let query =
            parse_query("SELECT DISTINCT file FROM functions GROUP BY file HAVING complexity > 10")
                .expect("query should parse");

        assert!(query.distinct);
        assert_eq!(query.group_by, vec!["file".to_string()]);
        assert_eq!(
            query.having,
            Some(Expr::Binary {
                left: Box::new(Expr::Identifier("complexity".to_string())),
                operator: BinaryOperator::Gt,
                right: Box::new(Expr::Literal(Literal::Integer(10))),
            })
        );
    }
}
