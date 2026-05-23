//! Shader source tokenizer for the Aster shading language.

/// Token produced by the shader tokenizer.
#[derive(Clone, Debug, PartialEq)]
pub enum ShaderToken {
    /// Identifier (variable, function, type name).
    Identifier(String),
    /// Language keyword.
    Keyword(ShaderKeyword),
    /// Numeric literal.
    Number(f32),
    /// Integer literal.
    Int(i64),
    /// String literal.
    String(String),
    /// Single-character symbol.
    Symbol(char),
    /// End of input.
    Eof,
}

/// Reserved keywords in the Aster shading language.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShaderKeyword {
    /// `fn` keyword.
    Fn,
    /// `struct` keyword.
    Struct,
    /// `let` keyword.
    Let,
    /// `var` keyword.
    Var,
    /// `if` keyword.
    If,
    /// `else` keyword.
    Else,
    /// `return` keyword.
    Return,
    /// `const` keyword.
    Const,
    /// `uniform` keyword.
    Uniform,
    /// `varying` keyword.
    Varying,
    /// `in` keyword.
    In,
    /// `out` keyword.
    Out,
    /// `for` keyword.
    For,
    /// `while` keyword.
    While,
    /// `break` keyword.
    Break,
    /// `continue` keyword.
    Continue,
    /// `true` keyword.
    True,
    /// `false` keyword.
    False,
    /// `import` keyword.
    Import,
}

/// Error during tokenization.
#[derive(Clone, Debug, PartialEq)]
pub struct TokenizeError {
    /// Error message.
    pub message: String,
    /// Approximate character position.
    pub position: usize,
}

/// Tokenizes a shader source string into a token stream.
pub fn tokenize(source: &str) -> Result<Vec<ShaderToken>, TokenizeError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut pos = 0;

    while pos < chars.len() {
        let ch = chars[pos];

        if ch.is_whitespace() {
            pos += 1;
            continue;
        }

        if ch == '/' && pos + 1 < chars.len() && chars[pos + 1] == '/' {
            while pos < chars.len() && chars[pos] != '\n' {
                pos += 1;
            }
            continue;
        }

        if ch == '/' && pos + 1 < chars.len() && chars[pos + 1] == '*' {
            pos += 2;
            while pos + 1 < chars.len() && !(chars[pos] == '*' && chars[pos + 1] == '/') {
                pos += 1;
            }
            pos += 2;
            continue;
        }

        if ch.is_alphabetic() || ch == '_' {
            let start = pos;
            while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
                pos += 1;
            }
            let ident: String = chars[start..pos].iter().collect();
            tokens.push(keyword_or_identifier(&ident));
            continue;
        }

        if ch.is_ascii_digit() || (ch == '.' && pos + 1 < chars.len() && chars[pos + 1].is_ascii_digit()) {
            let start = pos;
            let mut has_dot = false;
            while pos < chars.len() {
                let c = chars[pos];
                if c.is_ascii_digit() {
                    pos += 1;
                } else if c == '.' && !has_dot {
                    has_dot = true;
                    pos += 1;
                } else if c == 'e' || c == 'E' {
                    pos += 1;
                    if pos < chars.len() && (chars[pos] == '+' || chars[pos] == '-') {
                        pos += 1;
                    }
                } else {
                    break;
                }
            }
            let num_str: String = chars[start..pos].iter().collect();
            if has_dot {
                tokens.push(ShaderToken::Number(
                    num_str.parse().unwrap_or(0.0),
                ));
            } else {
                tokens.push(ShaderToken::Int(
                    num_str.parse().unwrap_or(0),
                ));
            }
            continue;
        }

        if ch == '"' {
            pos += 1;
            let start = pos;
            while pos < chars.len() && chars[pos] != '"' {
                if chars[pos] == '\\' {
                    pos += 1;
                }
                pos += 1;
            }
            let s: String = chars[start..pos].iter().collect();
            pos += 1;
            tokens.push(ShaderToken::String(s));
            continue;
        }

        let single_char_symbols = "{}()[];,.:+-*/%<>=!&|^~?@";
        if single_char_symbols.contains(ch) {
            tokens.push(ShaderToken::Symbol(ch));
            pos += 1;
            continue;
        }

        return Err(TokenizeError {
            message: format!("unexpected character: '{}'", ch),
            position: pos,
        });
    }

    tokens.push(ShaderToken::Eof);
    Ok(tokens)
}

fn keyword_or_identifier(word: &str) -> ShaderToken {
    match word {
        "fn" => ShaderToken::Keyword(ShaderKeyword::Fn),
        "struct" => ShaderToken::Keyword(ShaderKeyword::Struct),
        "let" => ShaderToken::Keyword(ShaderKeyword::Let),
        "var" => ShaderToken::Keyword(ShaderKeyword::Var),
        "if" => ShaderToken::Keyword(ShaderKeyword::If),
        "else" => ShaderToken::Keyword(ShaderKeyword::Else),
        "return" => ShaderToken::Keyword(ShaderKeyword::Return),
        "const" => ShaderToken::Keyword(ShaderKeyword::Const),
        "uniform" => ShaderToken::Keyword(ShaderKeyword::Uniform),
        "varying" => ShaderToken::Keyword(ShaderKeyword::Varying),
        "in" => ShaderToken::Keyword(ShaderKeyword::In),
        "out" => ShaderToken::Keyword(ShaderKeyword::Out),
        "for" => ShaderToken::Keyword(ShaderKeyword::For),
        "while" => ShaderToken::Keyword(ShaderKeyword::While),
        "break" => ShaderToken::Keyword(ShaderKeyword::Break),
        "continue" => ShaderToken::Keyword(ShaderKeyword::Continue),
        "true" => ShaderToken::Keyword(ShaderKeyword::True),
        "false" => ShaderToken::Keyword(ShaderKeyword::False),
        "import" => ShaderToken::Keyword(ShaderKeyword::Import),
        _ => ShaderToken::Identifier(word.to_string()),
    }
}
