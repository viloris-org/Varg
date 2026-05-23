//! Shader AST parser.

use crate::tokenizer::{ShaderKeyword, ShaderToken};

/// Top-level shader AST.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderAST {
    /// Top-level declarations.
    pub declarations: Vec<TopLevelDecl>,
}

/// A top-level declaration.
#[derive(Clone, Debug, PartialEq)]
pub enum TopLevelDecl {
    /// Struct definition.
    Struct(StructDecl),
    /// Function definition.
    Function(FunctionDecl),
    /// Global variable declaration.
    GlobalVar(GlobalVarDecl),
    /// Import declaration.
    Import(String),
}

/// Struct declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct StructDecl {
    /// Struct name.
    pub name: String,
    /// Struct fields.
    pub fields: Vec<(String, String)>,
}

/// Function declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionDecl {
    /// Function name.
    pub name: String,
    /// Return type.
    pub return_type: String,
    /// Parameters.
    pub params: Vec<(String, String)>,
    /// Function body statements.
    pub body: Vec<Stmt>,
}

/// Global variable declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct GlobalVarDecl {
    /// Variable name.
    pub name: String,
    /// Variable type.
    pub var_type: String,
    /// Whether this is a uniform.
    pub is_uniform: bool,
    /// Whether this is a varying.
    pub is_varying: bool,
}

/// A statement.
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// Variable declaration.
    Let {
        /// Variable name.
        name: String,
        /// Variable type.
        var_type: Option<String>,
        /// Initializer expression.
        init: Expr,
    },
    /// Return statement.
    Return(Option<Expr>),
    /// Expression statement.
    Expr(Expr),
    /// If statement.
    If {
        /// Condition.
        cond: Expr,
        /// Then block.
        then_body: Vec<Stmt>,
        /// Else block.
        else_body: Option<Vec<Stmt>>,
    },
    /// For statement.
    For {
        /// Loop variable.
        var: String,
        /// Start expression.
        start: Expr,
        /// End expression.
        end: Expr,
        /// Loop body.
        body: Vec<Stmt>,
    },
    /// Assignment.
    Assign {
        /// Target identifier.
        name: String,
        /// Value expression.
        value: Expr,
    },
}

/// An expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// Numeric literal.
    Number(f32),
    /// Integer literal.
    Int(i64),
    /// String literal.
    String(String),
    /// Boolean literal.
    Bool(bool),
    /// Identifier reference.
    Ident(String),
    /// Binary operation.
    Binary {
        /// Left operand.
        left: Box<Expr>,
        /// Operator.
        op: BinaryOp,
        /// Right operand.
        right: Box<Expr>,
    },
    /// Unary operation.
    Unary {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        expr: Box<Expr>,
    },
    /// Function call.
    Call {
        /// Function name.
        name: String,
        /// Arguments.
        args: Vec<Expr>,
    },
    /// Field access.
    Field {
        /// Object expression.
        expr: Box<Expr>,
        /// Field name.
        name: String,
    },
}

/// Binary operators.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    /// Addition.
    Add,
    /// Subtraction.
    Sub,
    /// Multiplication.
    Mul,
    /// Division.
    Div,
    /// Remainder.
    Rem,
    /// Equality.
    Eq,
    /// Inequality.
    Neq,
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Ge,
    /// Logical AND.
    And,
    /// Logical OR.
    Or,
}

/// Unary operators.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    /// Negation.
    Neg,
    /// Logical NOT.
    Not,
}

/// Error during parsing.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    /// Error message.
    pub message: String,
}

/// Parses tokens into a shader AST.
pub fn parse(tokens: &[ShaderToken]) -> Result<ShaderAST, ParseError> {
    let mut parser = Parser { tokens, pos: 0 };
    parser.parse_program()
}

struct Parser<'a> {
    tokens: &'a [ShaderToken],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn parse_program(&mut self) -> Result<ShaderAST, ParseError> {
        let mut declarations = Vec::new();
        while !self.is_eof() {
            declarations.push(self.parse_top_level()?);
        }
        Ok(ShaderAST { declarations })
    }

    fn parse_top_level(&mut self) -> Result<TopLevelDecl, ParseError> {
        match self.peek() {
            Some(ShaderToken::Keyword(ShaderKeyword::Struct)) => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect_symbol('{')?;
                let mut fields = Vec::new();
                while !self.check_symbol('}') {
                    let field_name = self.expect_ident()?;
                    self.expect_symbol(':')?;
                    let field_type = self.expect_ident()?;
                    fields.push((field_name, field_type));
                    if !self.check_symbol('}') {
                        self.expect_symbol(',')?;
                    }
                }
                self.expect_symbol('}')?;
                Ok(TopLevelDecl::Struct(StructDecl { name, fields }))
            }
            Some(ShaderToken::Keyword(ShaderKeyword::Fn)) => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect_symbol('(')?;
                let mut params = Vec::new();
                while !self.check_symbol(')') {
                    let param_name = self.expect_ident()?;
                    self.expect_symbol(':')?;
                    let param_type = self.expect_ident()?;
                    params.push((param_name, param_type));
                    if !self.check_symbol(')') {
                        self.expect_symbol(',')?;
                    }
                }
                self.expect_symbol(')')?;
                let return_type = if self.check_symbol(':') {
                    self.advance();
                    self.expect_ident()?
                } else if self.check_symbol('{') {
                    "void".to_string()
                } else {
                    return Err(ParseError {
                        message: "expected ':' or '{' after function params".to_string(),
                    });
                };
                self.expect_symbol('{')?;
                let body = self.parse_block()?;
                Ok(TopLevelDecl::Function(FunctionDecl {
                    name,
                    return_type,
                    params,
                    body,
                }))
            }
            Some(ShaderToken::Keyword(ShaderKeyword::Uniform))
            | Some(ShaderToken::Keyword(ShaderKeyword::Varying))
            | Some(ShaderToken::Keyword(ShaderKeyword::Const)) => {
                let is_uniform = matches!(self.peek(), Some(ShaderToken::Keyword(ShaderKeyword::Uniform)));
                let is_varying = matches!(self.peek(), Some(ShaderToken::Keyword(ShaderKeyword::Varying)));
                self.advance();
                let name = self.expect_ident()?;
                self.expect_symbol(':')?;
                let var_type = self.expect_ident()?;
                self.expect_symbol(';')?;
                Ok(TopLevelDecl::GlobalVar(GlobalVarDecl {
                    name,
                    var_type,
                    is_uniform,
                    is_varying,
                }))
            }
            Some(ShaderToken::Keyword(ShaderKeyword::Import)) => {
                self.advance();
                let path = self.expect_string()?;
                self.expect_symbol(';')?;
                Ok(TopLevelDecl::Import(path))
            }
            _ => Err(ParseError {
                message: "expected a top-level declaration".to_string(),
            }),
        }
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        while !self.check_symbol('}') && !self.is_eof() {
            stmts.push(self.parse_stmt()?);
        }
        self.expect_symbol('}')?;
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek() {
            Some(ShaderToken::Keyword(ShaderKeyword::Let)) => {
                self.advance();
                let name = self.expect_ident()?;
                let var_type = if self.check_symbol(':') {
                    self.advance();
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                self.expect_symbol('=')?;
                let init = self.parse_expr()?;
                self.expect_symbol(';')?;
                Ok(Stmt::Let {
                    name,
                    var_type,
                    init,
                })
            }
            Some(ShaderToken::Keyword(ShaderKeyword::Return)) => {
                self.advance();
                let expr = if self.check_symbol(';') {
                    None
                } else {
                    let e = self.parse_expr()?;
                    self.expect_symbol(';')?;
                    Some(e)
                };
                Ok(Stmt::Return(expr))
            }
            Some(ShaderToken::Keyword(ShaderKeyword::If)) => {
                self.advance();
                self.expect_symbol('(')?;
                let cond = self.parse_expr()?;
                self.expect_symbol(')')?;
                self.expect_symbol('{')?;
                let then_body = self.parse_block()?;
                let else_body = if self.check_keyword(ShaderKeyword::Else) {
                    self.advance();
                    self.expect_symbol('{')?;
                    Some(self.parse_block()?)
                } else {
                    None
                };
                Ok(Stmt::If {
                    cond,
                    then_body,
                    else_body,
                })
            }
            Some(ShaderToken::Keyword(ShaderKeyword::For)) => {
                self.advance();
                self.expect_symbol('(')?;
                let var = self.expect_ident()?;
                self.expect_symbol('=')?;
                let start = self.parse_expr()?;
                self.expect_symbol(';')?;
                let end = self.parse_expr()?;
                self.expect_symbol(')')?;
                self.expect_symbol('{')?;
                let body = self.parse_block()?;
                Ok(Stmt::For {
                    var,
                    start,
                    end,
                    body,
                })
            }
            _ => {
                let expr = self.parse_expr()?;
                if self.check_symbol('=') {
                    self.advance();
                    let value = self.parse_expr()?;
                    self.expect_symbol(';')?;
                    if let Expr::Ident(name) = expr {
                        return Ok(Stmt::Assign { name, value });
                    }
                    return Err(ParseError {
                        message: "assignment target must be an identifier".to_string(),
                    });
                }
                self.expect_symbol(';')?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_binary(0)
    }

    fn parse_binary(&mut self, min_prec: u8) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let token = self.peek().cloned();
            let op = match token {
                Some(ShaderToken::Symbol('+')) => Some((BinaryOp::Add, 1)),
                Some(ShaderToken::Symbol('-')) => Some((BinaryOp::Sub, 1)),
                Some(ShaderToken::Symbol('*')) => Some((BinaryOp::Mul, 2)),
                Some(ShaderToken::Symbol('/')) => Some((BinaryOp::Div, 2)),
                Some(ShaderToken::Symbol('%')) => Some((BinaryOp::Rem, 2)),
                Some(ShaderToken::Symbol('<')) => {
                    if self.check_future_symbol('=') {
                        Some((BinaryOp::Le, 0))
                    } else {
                        Some((BinaryOp::Lt, 0))
                    }
                }
                Some(ShaderToken::Symbol('>')) => {
                    if self.check_future_symbol('=') {
                        Some((BinaryOp::Ge, 0))
                    } else {
                        Some((BinaryOp::Gt, 0))
                    }
                }
                Some(ShaderToken::Symbol('=')) => {
                    if self.check_future_symbol('=') {
                        Some((BinaryOp::Eq, 0))
                    } else {
                        None
                    }
                }
                Some(ShaderToken::Symbol('!')) => {
                    if self.check_future_symbol('=') {
                        Some((BinaryOp::Neq, 0))
                    } else {
                        None
                    }
                }
                Some(ShaderToken::Symbol('&')) => {
                    if self.check_future_symbol('&') {
                        Some((BinaryOp::And, 0))
                    } else {
                        None
                    }
                }
                Some(ShaderToken::Symbol('|')) => {
                    if self.check_future_symbol('|') {
                        Some((BinaryOp::Or, 0))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let Some(op) = op else { break };
            if op.1 < min_prec {
                break;
            }
            self.advance();
            if matches!(op.0, BinaryOp::Le | BinaryOp::Ge | BinaryOp::Eq | BinaryOp::Neq | BinaryOp::And | BinaryOp::Or) {
                self.advance();
            }
            let right = self.parse_binary(op.1 + 1)?;
            left = Expr::Binary {
                left: Box::new(left),
                op: op.0,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().cloned() {
            Some(ShaderToken::Symbol('-')) => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            Some(ShaderToken::Symbol('!')) => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(self.parse_unary()?),
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let token = self.peek().cloned();
        let expr = match token {
            Some(ShaderToken::Number(n)) => {
                self.advance();
                Expr::Number(n)
            }
            Some(ShaderToken::Int(n)) => {
                self.advance();
                Expr::Int(n)
            }
            Some(ShaderToken::String(s)) => {
                self.advance();
                Expr::String(s)
            }
            Some(ShaderToken::Keyword(ShaderKeyword::True)) => {
                self.advance();
                Expr::Bool(true)
            }
            Some(ShaderToken::Keyword(ShaderKeyword::False)) => {
                self.advance();
                Expr::Bool(false)
            }
            Some(ShaderToken::Identifier(_)) => {
                let name = self.expect_ident()?;
                if self.check_symbol('(') {
                    self.advance();
                    let mut args = Vec::new();
                    while !self.check_symbol(')') {
                        args.push(self.parse_expr()?);
                        if !self.check_symbol(')') {
                            self.expect_symbol(',')?;
                        }
                    }
                    self.expect_symbol(')')?;
                    Expr::Call { name, args }
                } else {
                    Expr::Ident(name)
                }
            }
            Some(ShaderToken::Symbol('(')) => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect_symbol(')')?;
                expr
            }
            _ => {
                return Err(ParseError {
                    message: "expected an expression".to_string(),
                })
            }
        };

        let mut expr = expr;
        while self.check_symbol('.') {
            self.advance();
            let name = self.expect_ident()?;
            expr = Expr::Field {
                expr: Box::new(expr),
                name,
            };
        }
        Ok(expr)
    }

    fn peek(&self) -> Option<&ShaderToken> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&ShaderToken> {
        let tok = self.tokens.get(self.pos);
        self.pos += 1;
        tok
    }

    fn is_eof(&self) -> bool {
        matches!(self.peek(), Some(ShaderToken::Eof) | None)
    }

    fn check_symbol(&self, ch: char) -> bool {
        matches!(self.peek(), Some(ShaderToken::Symbol(c)) if *c == ch)
    }

    fn check_future_symbol(&self, ch: char) -> bool {
        matches!(self.tokens.get(self.pos + 1), Some(ShaderToken::Symbol(c)) if *c == ch)
    }

    fn check_keyword(&self, kw: ShaderKeyword) -> bool {
        matches!(self.peek(), Some(ShaderToken::Keyword(k)) if *k == kw)
    }

    fn expect_symbol(&mut self, ch: char) -> Result<(), ParseError> {
        match self.peek() {
            Some(ShaderToken::Symbol(c)) if *c == ch => {
                self.advance();
                Ok(())
            }
            _ => Err(ParseError {
                message: format!("expected '{}'", ch),
            }),
        }
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Some(ShaderToken::Identifier(s)) => Ok(s.clone()),
            _ => Err(ParseError {
                message: "expected identifier".to_string(),
            }),
        }
    }

    fn expect_string(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Some(ShaderToken::String(s)) => Ok(s.clone()),
            _ => Err(ParseError {
                message: "expected string literal".to_string(),
            }),
        }
    }
}
