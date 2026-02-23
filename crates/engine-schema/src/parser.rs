/// Recursive-descent parser for the ECS IDL.
use crate::ast::*;
use crate::lexer::{LexError, Lexer, SpannedToken, Token};
use std::fmt;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        Self {
            line: e.line,
            col: e.col,
            message: e.message,
        }
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    pub fn parse(input: &str) -> Result<File, ParseError> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        let mut parser = Self { tokens, pos: 0 };
        parser.parse_file()
    }

    // -- Helpers --

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn current_span(&self) -> (usize, usize) {
        let t = &self.tokens[self.pos];
        (t.line, t.col)
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos].token;
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            let (line, col) = self.current_span();
            Err(ParseError {
                line,
                col,
                message: format!("expected {expected}, got {}", self.peek()),
            })
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.advance();
                Ok(s)
            }
            // Allow keywords that can also be used as identifiers in certain positions
            Token::Read
            | Token::Write
            | Token::Optional
            | Token::Exclude
            | Token::Changed
            | Token::Hz => {
                let s = self.peek().to_string();
                self.advance();
                Ok(s)
            }
            other => {
                let (line, col) = self.current_span();
                Err(ParseError {
                    line,
                    col,
                    message: format!("expected identifier, got {other}"),
                })
            }
        }
    }

    fn at(&self, token: &Token) -> bool {
        self.peek() == token
    }

    fn eat(&mut self, token: &Token) -> bool {
        if self.at(token) {
            self.advance();
            true
        } else {
            false
        }
    }

    // -- Top-level --

    fn parse_file(&mut self) -> Result<File, ParseError> {
        let package = self.parse_package_decl()?;
        let mut imports = Vec::new();
        while self.at(&Token::Use) {
            imports.push(self.parse_import()?);
        }
        let mut items = Vec::new();
        while !self.at(&Token::Eof) {
            items.push(self.parse_top_level_item()?);
        }
        Ok(File {
            package,
            imports,
            items,
        })
    }

    // -- Package --

    fn parse_package_decl(&mut self) -> Result<PackageDecl, ParseError> {
        self.expect(&Token::Package)?;
        let namespace = self.expect_ident()?;
        self.expect(&Token::Colon)?;
        let name = self.expect_ident()?;
        let version = if self.eat(&Token::At) {
            Some(self.parse_version()?)
        } else {
            None
        };
        Ok(PackageDecl {
            namespace,
            name,
            version,
        })
    }

    fn parse_version(&mut self) -> Result<String, ParseError> {
        let major = self.expect_integer()?;
        self.expect(&Token::Dot)?;
        let minor = self.expect_integer()?;
        self.expect(&Token::Dot)?;
        let patch = self.expect_integer()?;
        Ok(format!("{major}.{minor}.{patch}"))
    }

    fn expect_integer(&mut self) -> Result<u64, ParseError> {
        match self.peek().clone() {
            Token::Integer(n) => {
                self.advance();
                Ok(n)
            }
            other => {
                let (line, col) = self.current_span();
                Err(ParseError {
                    line,
                    col,
                    message: format!("expected integer, got {other}"),
                })
            }
        }
    }

    // -- Import --

    fn parse_import(&mut self) -> Result<Import, ParseError> {
        self.expect(&Token::Use)?;
        let package = self.parse_package_ref()?;
        self.expect(&Token::Dot)?;
        self.expect(&Token::LBrace)?;
        let items = self.parse_import_list()?;
        self.expect(&Token::RBrace)?;
        Ok(Import { package, items })
    }

    fn parse_package_ref(&mut self) -> Result<PackageRef, ParseError> {
        let namespace = self.expect_ident()?;
        self.expect(&Token::Colon)?;
        let name = self.expect_ident()?;
        let version = if self.eat(&Token::At) {
            Some(self.parse_version()?)
        } else {
            None
        };
        Ok(PackageRef {
            namespace,
            name,
            version,
        })
    }

    fn parse_import_list(&mut self) -> Result<Vec<ImportItem>, ParseError> {
        let mut items = Vec::new();
        loop {
            if self.at(&Token::RBrace) {
                break;
            }
            let name = self.expect_ident()?;
            let alias = if self.eat(&Token::As) {
                Some(self.expect_ident()?)
            } else {
                None
            };
            items.push(ImportItem { name, alias });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(items)
    }

    // -- Top-level items --

    fn parse_top_level_item(&mut self) -> Result<TopLevelItem, ParseError> {
        match self.peek() {
            Token::Type => Ok(TopLevelItem::TypeAlias(self.parse_type_alias()?)),
            Token::Enum => Ok(TopLevelItem::Enum(self.parse_enum()?)),
            Token::Variant => Ok(TopLevelItem::Variant(self.parse_variant()?)),
            Token::Flags => Ok(TopLevelItem::Flags(self.parse_flags()?)),
            Token::Record => Ok(TopLevelItem::Record(self.parse_record()?)),
            Token::System => Ok(TopLevelItem::System(self.parse_system()?)),
            Token::Phase => Ok(TopLevelItem::Phase(self.parse_phase()?)),
            Token::World => Ok(TopLevelItem::World(self.parse_world()?)),
            other => {
                let (line, col) = self.current_span();
                Err(ParseError {
                    line,
                    col,
                    message: format!("expected top-level item, got {other}"),
                })
            }
        }
    }

    // -- Type alias --

    fn parse_type_alias(&mut self) -> Result<TypeAlias, ParseError> {
        self.expect(&Token::Type)?;
        let name = self.expect_ident()?;
        self.expect(&Token::Eq)?;
        let target = self.parse_type_expr()?;
        Ok(TypeAlias { name, target })
    }

    // -- Type expression --

    fn parse_type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        match self.peek().clone() {
            Token::List => {
                self.advance();
                self.expect(&Token::LAngle)?;
                let inner = self.parse_type_expr()?;
                self.expect(&Token::RAngle)?;
                Ok(TypeExpr::List(Box::new(inner)))
            }
            Token::OptionKw => {
                self.advance();
                self.expect(&Token::LAngle)?;
                let inner = self.parse_type_expr()?;
                self.expect(&Token::RAngle)?;
                Ok(TypeExpr::Option(Box::new(inner)))
            }
            Token::Set => {
                self.advance();
                self.expect(&Token::LAngle)?;
                let inner = self.parse_type_expr()?;
                self.expect(&Token::RAngle)?;
                Ok(TypeExpr::Set(Box::new(inner)))
            }
            Token::Map => {
                self.advance();
                self.expect(&Token::LAngle)?;
                let key = self.parse_type_expr()?;
                self.expect(&Token::Comma)?;
                let val = self.parse_type_expr()?;
                self.expect(&Token::RAngle)?;
                Ok(TypeExpr::Map(Box::new(key), Box::new(val)))
            }
            Token::Tuple => {
                self.advance();
                self.expect(&Token::LAngle)?;
                let mut types = vec![self.parse_type_expr()?];
                while self.eat(&Token::Comma) {
                    if self.at(&Token::RAngle) {
                        break;
                    }
                    types.push(self.parse_type_expr()?);
                }
                self.expect(&Token::RAngle)?;
                Ok(TypeExpr::Tuple(types))
            }
            Token::Ident(ref s) => {
                let name = s.clone();
                self.advance();
                if is_primitive(&name) {
                    Ok(TypeExpr::Primitive(name))
                } else {
                    Ok(TypeExpr::Named(name))
                }
            }
            other => {
                let (line, col) = self.current_span();
                Err(ParseError {
                    line,
                    col,
                    message: format!("expected type expression, got {other}"),
                })
            }
        }
    }

    // -- Enum --

    fn parse_enum(&mut self) -> Result<EnumDef, ParseError> {
        self.expect(&Token::Enum)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LBrace)?;
        let mut variants = Vec::new();
        loop {
            if self.at(&Token::RBrace) {
                break;
            }
            variants.push(self.expect_ident()?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(EnumDef { name, variants })
    }

    // -- Variant --

    fn parse_variant(&mut self) -> Result<VariantDef, ParseError> {
        self.expect(&Token::Variant)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LBrace)?;
        let mut cases = Vec::new();
        loop {
            if self.at(&Token::RBrace) {
                break;
            }
            let case_name = self.expect_ident()?;
            let payload = if self.eat(&Token::LParen) {
                let mut types = vec![self.parse_type_expr()?];
                while self.eat(&Token::Comma) {
                    if self.at(&Token::RParen) {
                        break;
                    }
                    types.push(self.parse_type_expr()?);
                }
                self.expect(&Token::RParen)?;
                Some(types)
            } else {
                None
            };
            cases.push(VariantCase {
                name: case_name,
                payload,
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(VariantDef { name, cases })
    }

    // -- Flags --

    fn parse_flags(&mut self) -> Result<FlagsDef, ParseError> {
        self.expect(&Token::Flags)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LBrace)?;
        let mut flags = Vec::new();
        loop {
            if self.at(&Token::RBrace) {
                break;
            }
            flags.push(self.expect_ident()?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(FlagsDef { name, flags })
    }

    // -- Record --

    fn parse_record(&mut self) -> Result<RecordDef, ParseError> {
        self.expect(&Token::Record)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        loop {
            if self.at(&Token::RBrace) {
                break;
            }
            let field_name = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let ty = self.parse_type_expr()?;
            fields.push(Field {
                name: field_name,
                ty,
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(RecordDef { name, fields })
    }

    // -- Phase --

    fn parse_phase(&mut self) -> Result<PhaseDef, ParseError> {
        self.expect(&Token::Phase)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LBrace)?;
        let mut hz = None;
        if self.at(&Token::Hz) {
            self.advance();
            self.expect(&Token::Colon)?;
            hz = Some(self.expect_integer()? as u32);
            self.eat(&Token::Comma);
        }
        self.expect(&Token::RBrace)?;
        Ok(PhaseDef { name, hz })
    }

    // -- System --

    fn parse_system(&mut self) -> Result<SystemDef, ParseError> {
        self.expect(&Token::System)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LBrace)?;

        let mut queries = Vec::new();
        let mut phase = None;
        let mut order_after = Vec::new();
        let mut order_before = Vec::new();

        while !self.at(&Token::RBrace) {
            match self.peek() {
                Token::Query => {
                    queries.push(self.parse_query()?);
                }
                Token::Phase => {
                    self.advance();
                    self.expect(&Token::Colon)?;
                    phase = Some(self.expect_ident()?);
                    self.eat(&Token::Comma);
                }
                Token::OrderAfter => {
                    self.advance();
                    self.expect(&Token::Colon)?;
                    order_after = self.parse_ident_list()?;
                    self.eat(&Token::Comma);
                }
                Token::OrderBefore => {
                    self.advance();
                    self.expect(&Token::Colon)?;
                    order_before = self.parse_ident_list()?;
                    self.eat(&Token::Comma);
                }
                other => {
                    let (line, col) = self.current_span();
                    return Err(ParseError {
                        line,
                        col,
                        message: format!("unexpected token in system body: {other}"),
                    });
                }
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(SystemDef {
            name,
            queries,
            phase,
            order_after,
            order_before,
        })
    }

    fn parse_query(&mut self) -> Result<QueryDef, ParseError> {
        self.expect(&Token::Query)?;

        // Optional query name
        let name = if !self.at(&Token::LBrace) {
            Some(self.expect_ident()?)
        } else {
            None
        };

        self.expect(&Token::LBrace)?;

        let mut read = Vec::new();
        let mut write = Vec::new();
        let mut optional = Vec::new();
        let mut exclude = Vec::new();
        let mut changed = Vec::new();

        while !self.at(&Token::RBrace) {
            match self.peek() {
                Token::Read => {
                    self.advance();
                    self.expect(&Token::Colon)?;
                    read = self.parse_ident_list()?;
                    self.eat(&Token::Comma);
                }
                Token::Write => {
                    self.advance();
                    self.expect(&Token::Colon)?;
                    write = self.parse_ident_list()?;
                    self.eat(&Token::Comma);
                }
                Token::Optional => {
                    self.advance();
                    self.expect(&Token::Colon)?;
                    optional = self.parse_ident_list()?;
                    self.eat(&Token::Comma);
                }
                Token::Exclude => {
                    self.advance();
                    self.expect(&Token::Colon)?;
                    exclude = self.parse_ident_list()?;
                    self.eat(&Token::Comma);
                }
                Token::Changed => {
                    self.advance();
                    self.expect(&Token::Colon)?;
                    changed = self.parse_ident_list()?;
                    self.eat(&Token::Comma);
                }
                other => {
                    let (line, col) = self.current_span();
                    return Err(ParseError {
                        line,
                        col,
                        message: format!("unexpected token in query body: {other}"),
                    });
                }
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(QueryDef {
            name,
            read,
            write,
            optional,
            exclude,
            changed,
        })
    }

    fn parse_ident_list(&mut self) -> Result<Vec<String>, ParseError> {
        self.expect(&Token::LBracket)?;
        let mut items = Vec::new();
        loop {
            if self.at(&Token::RBracket) {
                break;
            }
            items.push(self.expect_ident()?);
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBracket)?;
        Ok(items)
    }

    // -- World --

    fn parse_world(&mut self) -> Result<WorldDef, ParseError> {
        self.expect(&Token::World)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LBrace)?;

        let mut includes = Vec::new();
        let mut items = Vec::new();

        while !self.at(&Token::RBrace) {
            if self.at(&Token::Include) {
                self.advance();
                let package = self.parse_package_ref()?;
                let item = if self.eat(&Token::Dot) {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                includes.push(IncludeStmt { package, item });
            } else {
                items.push(self.parse_top_level_item()?);
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(WorldDef {
            name,
            includes,
            items,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_primitive(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "f32"
            | "f64"
            | "string"
            | "bytes"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal() {
        let input = r#"
            package test:minimal@0.1.0

            record empty {}
        "#;
        let file = Parser::parse(input).unwrap();
        assert_eq!(file.package.namespace, "test");
        assert_eq!(file.package.name, "minimal");
        assert_eq!(file.items.len(), 1);
    }

    #[test]
    fn test_parse_record_with_fields() {
        let input = r#"
            package test:records@0.1.0

            record transform {
                x: f32,
                y: f32,
                z: f32,
            }
        "#;
        let file = Parser::parse(input).unwrap();
        if let TopLevelItem::Record(rec) = &file.items[0] {
            assert_eq!(rec.name, "transform");
            assert_eq!(rec.fields.len(), 3);
            assert!(!rec.is_tag());
        } else {
            panic!("expected record");
        }
    }

    #[test]
    fn test_parse_tag() {
        let input = r#"
            package test:tags@0.1.0
            record frozen {}
        "#;
        let file = Parser::parse(input).unwrap();
        if let TopLevelItem::Record(rec) = &file.items[0] {
            assert!(rec.is_tag());
        } else {
            panic!("expected record");
        }
    }

    #[test]
    fn test_parse_system() {
        let input = r#"
            package test:systems@0.1.0

            system physics {
                query {
                    read: [velocity, mass],
                    write: [transform],
                    exclude: [frozen],
                }
                phase: fixed_update,
                order_after: [input],
            }
        "#;
        let file = Parser::parse(input).unwrap();
        if let TopLevelItem::System(sys) = &file.items[0] {
            assert_eq!(sys.name, "physics");
            assert_eq!(sys.queries.len(), 1);
            assert_eq!(sys.queries[0].read, vec!["velocity", "mass"]);
            assert_eq!(sys.queries[0].write, vec!["transform"]);
            assert_eq!(sys.queries[0].exclude, vec!["frozen"]);
            assert_eq!(sys.phase, Some("fixed_update".to_string()));
        } else {
            panic!("expected system");
        }
    }

    #[test]
    fn test_parse_imports() {
        let input = r#"
            package test:imports@0.1.0

            use engine:std.{vec3, quat, entity_id}
            use engine:physics.{transform as t, velocity}

            record foo {
                pos: vec3,
            }
        "#;
        let file = Parser::parse(input).unwrap();
        assert_eq!(file.imports.len(), 2);
        assert_eq!(file.imports[0].items.len(), 3);
        assert_eq!(file.imports[1].items[0].alias, Some("t".to_string()));
    }

    #[test]
    fn test_parse_enum_variant_flags() {
        let input = r#"
            package test:valuetypes@0.1.0

            enum color { red, green, blue }

            variant shape {
                circle(f32),
                rect(f32, f32),
                none,
            }

            flags layers {
                terrain,
                objects,
                effects,
            }
        "#;
        let file = Parser::parse(input).unwrap();
        assert_eq!(file.items.len(), 3);
    }
}
