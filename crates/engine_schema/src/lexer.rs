/// Lexer for the ECS IDL.
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Package,
    Use,
    Type,
    Enum,
    Variant,
    Flags,
    Record,
    System,
    Phase,
    World,
    Query,
    Read,
    Write,
    Optional,
    Exclude,
    Changed,
    Include,
    As,
    OrderAfter,
    OrderBefore,
    Hz,

    // Parameterized type keywords
    List,
    OptionKw,
    Set,
    Map,
    Tuple,

    // Literals
    Ident(String),
    Integer(u64),

    // Punctuation
    Colon,
    Comma,
    Dot,
    At,
    Eq,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LAngle,
    RAngle,
    LParen,
    RParen,

    // Special
    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Package => write!(f, "package"),
            Token::Use => write!(f, "use"),
            Token::Type => write!(f, "type"),
            Token::Enum => write!(f, "enum"),
            Token::Variant => write!(f, "variant"),
            Token::Flags => write!(f, "flags"),
            Token::Record => write!(f, "record"),
            Token::System => write!(f, "system"),
            Token::Phase => write!(f, "phase"),
            Token::World => write!(f, "world"),
            Token::Query => write!(f, "query"),
            Token::Read => write!(f, "read"),
            Token::Write => write!(f, "write"),
            Token::Optional => write!(f, "optional"),
            Token::Exclude => write!(f, "exclude"),
            Token::Changed => write!(f, "changed"),
            Token::Include => write!(f, "include"),
            Token::As => write!(f, "as"),
            Token::OrderAfter => write!(f, "order_after"),
            Token::OrderBefore => write!(f, "order_before"),
            Token::Hz => write!(f, "hz"),
            Token::List => write!(f, "list"),
            Token::OptionKw => write!(f, "option"),
            Token::Set => write!(f, "set"),
            Token::Map => write!(f, "map"),
            Token::Tuple => write!(f, "tuple"),
            Token::Ident(s) => write!(f, "{s}"),
            Token::Integer(n) => write!(f, "{n}"),
            Token::Colon => write!(f, ":"),
            Token::Comma => write!(f, ","),
            Token::Dot => write!(f, "."),
            Token::At => write!(f, "@"),
            Token::Eq => write!(f, "="),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LAngle => write!(f, "<"),
            Token::RAngle => write!(f, ">"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::Eof => write!(f, "EOF"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub line: usize,
    pub col: usize,
}

pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<SpannedToken>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.token == Token::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek_byte(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while let Some(b) = self.peek_byte() {
                if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                    self.advance();
                } else {
                    break;
                }
            }

            // Skip line comments
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'/'
                && self.input[self.pos + 1] == b'/'
            {
                while let Some(b) = self.peek_byte() {
                    self.advance();
                    if b == b'\n' {
                        break;
                    }
                }
                continue;
            }

            // Skip block comments
            if self.pos + 1 < self.input.len()
                && self.input[self.pos] == b'/'
                && self.input[self.pos + 1] == b'*'
            {
                self.advance(); // /
                self.advance(); // *
                loop {
                    match self.advance() {
                        None => break,
                        Some(b'*') => {
                            if self.peek_byte() == Some(b'/') {
                                self.advance();
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                continue;
            }

            break;
        }
    }

    fn next_token(&mut self) -> Result<SpannedToken, LexError> {
        self.skip_whitespace_and_comments();

        let line = self.line;
        let col = self.col;

        let Some(b) = self.peek_byte() else {
            return Ok(SpannedToken {
                token: Token::Eof,
                line,
                col,
            });
        };

        // Punctuation
        let punct = match b {
            b':' => Some(Token::Colon),
            b',' => Some(Token::Comma),
            b'.' => Some(Token::Dot),
            b'@' => Some(Token::At),
            b'=' => Some(Token::Eq),
            b'{' => Some(Token::LBrace),
            b'}' => Some(Token::RBrace),
            b'[' => Some(Token::LBracket),
            b']' => Some(Token::RBracket),
            b'<' => Some(Token::LAngle),
            b'>' => Some(Token::RAngle),
            b'(' => Some(Token::LParen),
            b')' => Some(Token::RParen),
            _ => None,
        };

        if let Some(token) = punct {
            self.advance();
            return Ok(SpannedToken { token, line, col });
        }

        // Numbers
        if b.is_ascii_digit() {
            let mut num = 0u64;
            while let Some(d) = self.peek_byte() {
                if d.is_ascii_digit() {
                    num = num * 10 + (d - b'0') as u64;
                    self.advance();
                } else {
                    break;
                }
            }
            return Ok(SpannedToken {
                token: Token::Integer(num),
                line,
                col,
            });
        }

        // Identifiers and keywords
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = self.pos;
            while let Some(c) = self.peek_byte() {
                if c.is_ascii_alphanumeric() || c == b'_' {
                    self.advance();
                } else {
                    break;
                }
            }
            let word = std::str::from_utf8(&self.input[start..self.pos]).unwrap();
            let token = match word {
                "package" => Token::Package,
                "use" => Token::Use,
                "type" => Token::Type,
                "enum" => Token::Enum,
                "variant" => Token::Variant,
                "flags" => Token::Flags,
                "record" => Token::Record,
                "system" => Token::System,
                "phase" => Token::Phase,
                "world" => Token::World,
                "query" => Token::Query,
                "read" => Token::Read,
                "write" => Token::Write,
                "optional" => Token::Optional,
                "exclude" => Token::Exclude,
                "changed" => Token::Changed,
                "include" => Token::Include,
                "as" => Token::As,
                "order_after" => Token::OrderAfter,
                "order_before" => Token::OrderBefore,
                "hz" => Token::Hz,
                "list" => Token::List,
                "option" => Token::OptionKw,
                "set" => Token::Set,
                "map" => Token::Map,
                "tuple" => Token::Tuple,
                other => Token::Ident(other.to_string()),
            };
            return Ok(SpannedToken { token, line, col });
        }

        Err(LexError {
            line,
            col,
            message: format!("unexpected character: '{}'", b as char),
        })
    }
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let input = "record vec3 { x: f32, y: f32, z: f32 }";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Record);
        assert!(matches!(tokens[1].token, Token::Ident(ref s) if s == "vec3"));
        assert_eq!(tokens[2].token, Token::LBrace);
    }

    #[test]
    fn test_comments() {
        let input = "// line comment\nrecord /* block */ foo {}";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Record);
        assert!(matches!(tokens[1].token, Token::Ident(ref s) if s == "foo"));
    }

    #[test]
    fn test_package_ref() {
        let input = "package engine:std@0.1.0";
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].token, Token::Package);
        assert!(matches!(tokens[1].token, Token::Ident(ref s) if s == "engine"));
        assert_eq!(tokens[2].token, Token::Colon);
    }
}
