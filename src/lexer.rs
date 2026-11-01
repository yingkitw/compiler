//! Tokenizer for the TypeScript subset.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Let,
    Const,
    Function,
    If,
    Else,
    While,
    Return,
    True,
    False,
    Ident(String),
    Num(f64),
    LParen,
    RParen,
    LBrace,
    RBrace,
    Semi,
    Comma,
    Colon,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    Bang,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

pub fn lex(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;

    macro_rules! err {
        ($($arg:tt)*) => { return Err(format!("line {}: {}", line, format!($($arg)*))) };
    }

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                loop {
                    if i >= chars.len() {
                        err!("unterminated block comment");
                    }
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            c if c.is_ascii_alphabetic() || c == '_' || c == '$' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let tok = match word.as_str() {
                    "let" => Tok::Let,
                    "const" => Tok::Const,
                    "function" => Tok::Function,
                    "if" => Tok::If,
                    "else" => Tok::Else,
                    "while" => Tok::While,
                    "return" => Tok::Return,
                    "true" => Tok::True,
                    "false" => Tok::False,
                    _ => Tok::Ident(word),
                };
                out.push(Token { tok, line });
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                match text.parse::<f64>() {
                    Ok(n) => out.push(Token { tok: Tok::Num(n), line }),
                    Err(_) => err!("invalid number literal '{}'", text),
                }
            }
            '(' => {
                out.push(Token { tok: Tok::LParen, line });
                i += 1;
            }
            ')' => {
                out.push(Token { tok: Tok::RParen, line });
                i += 1;
            }
            '{' => {
                out.push(Token { tok: Tok::LBrace, line });
                i += 1;
            }
            '}' => {
                out.push(Token { tok: Tok::RBrace, line });
                i += 1;
            }
            ';' => {
                out.push(Token { tok: Tok::Semi, line });
                i += 1;
            }
            ',' => {
                out.push(Token { tok: Tok::Comma, line });
                i += 1;
            }
            ':' => {
                out.push(Token { tok: Tok::Colon, line });
                i += 1;
            }
            '+' => {
                out.push(Token { tok: Tok::Plus, line });
                i += 1;
            }
            '-' => {
                out.push(Token { tok: Tok::Minus, line });
                i += 1;
            }
            '*' => {
                out.push(Token { tok: Tok::Star, line });
                i += 1;
            }
            '/' => {
                out.push(Token { tok: Tok::Slash, line });
                i += 1;
            }
            '=' if chars.get(i + 1) == Some(&'=') => {
                out.push(Token { tok: Tok::EqEq, line });
                i += 2;
            }
            '=' => {
                out.push(Token { tok: Tok::Assign, line });
                i += 1;
            }
            '!' if chars.get(i + 1) == Some(&'=') => {
                out.push(Token { tok: Tok::BangEq, line });
                i += 2;
            }
            '!' => {
                out.push(Token { tok: Tok::Bang, line });
                i += 1;
            }
            '<' if chars.get(i + 1) == Some(&'=') => {
                out.push(Token { tok: Tok::LtEq, line });
                i += 2;
            }
            '<' => {
                out.push(Token { tok: Tok::Lt, line });
                i += 1;
            }
            '>' if chars.get(i + 1) == Some(&'=') => {
                out.push(Token { tok: Tok::GtEq, line });
                i += 2;
            }
            '>' => {
                out.push(Token { tok: Tok::Gt, line });
                i += 1;
            }
            '&' if chars.get(i + 1) == Some(&'&') => {
                out.push(Token { tok: Tok::AndAnd, line });
                i += 2;
            }
            '|' if chars.get(i + 1) == Some(&'|') => {
                out.push(Token { tok: Tok::OrOr, line });
                i += 2;
            }
            _ => err!("unexpected character '{}'", c),
        }
    }
    out.push(Token { tok: Tok::Eof, line });
    Ok(out)
}
