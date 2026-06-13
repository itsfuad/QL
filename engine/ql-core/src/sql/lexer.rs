#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
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
    Identifier(String),
    Integer(u64),
    String(String),
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

pub fn lex(input: &str) -> Result<Vec<Token>, usize> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        let start = index;
        let kind = match byte {
            b',' => {
                index += 1;
                TokenKind::Comma
            }
            b'.' => {
                index += 1;
                TokenKind::Dot
            }
            b'(' => {
                index += 1;
                TokenKind::LParen
            }
            b')' => {
                index += 1;
                TokenKind::RParen
            }
            b'*' => {
                index += 1;
                TokenKind::Star
            }
            b'=' => {
                index += 1;
                TokenKind::Eq
            }
            b'>' => {
                index += 1;
                if bytes.get(index) == Some(&b'=') {
                    index += 1;
                    TokenKind::Gte
                } else {
                    TokenKind::Gt
                }
            }
            b'<' => {
                index += 1;
                match bytes.get(index) {
                    Some(b'=') => {
                        index += 1;
                        TokenKind::Lte
                    }
                    Some(b'>') => {
                        index += 1;
                        TokenKind::NotEq
                    }
                    _ => TokenKind::Lt,
                }
            }
            b'!' => {
                if bytes.get(index + 1) != Some(&b'=') {
                    return Err(start);
                }
                index += 2;
                TokenKind::NotEq
            }
            b'\'' => {
                index += 1;
                let mut value = String::new();
                while let Some(&next) = bytes.get(index) {
                    if next == b'\'' {
                        if bytes.get(index + 1) == Some(&b'\'') {
                            value.push('\'');
                            index += 2;
                            continue;
                        }
                        index += 1;
                        break;
                    }
                    value.push(next as char);
                    index += 1;
                }
                if !matches!(bytes.get(index.wrapping_sub(1)), Some(b'\'')) {
                    return Err(start);
                }
                TokenKind::String(value)
            }
            b'0'..=b'9' => {
                while matches!(bytes.get(index), Some(b'0'..=b'9')) {
                    index += 1;
                }
                let value = input[start..index].parse().map_err(|_| start)?;
                TokenKind::Integer(value)
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                while matches!(
                    bytes.get(index),
                    Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
                ) {
                    index += 1;
                }
                keyword_or_identifier(&input[start..index])
            }
            _ => return Err(start),
        };

        tokens.push(Token { kind, start });
    }

    Ok(tokens)
}

fn keyword_or_identifier(value: &str) -> TokenKind {
    match value.to_ascii_uppercase().as_str() {
        "SELECT" => TokenKind::Select,
        "FROM" => TokenKind::From,
        "JOIN" => TokenKind::Join,
        "ON" => TokenKind::On,
        "WHERE" => TokenKind::Where,
        "ORDER" => TokenKind::Order,
        "BY" => TokenKind::By,
        "LIMIT" => TokenKind::Limit,
        "ASC" => TokenKind::Asc,
        "DESC" => TokenKind::Desc,
        "AND" => TokenKind::And,
        "OR" => TokenKind::Or,
        "NOT" => TokenKind::Not,
        "IN" => TokenKind::In,
        "LIKE" => TokenKind::Like,
        _ => TokenKind::Identifier(value.to_string()),
    }
}
