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

#[cfg(test)]
mod tests {
    use super::*;

    /// Token kinds only, dropping line numbers and the trailing EOF.
    fn kinds(src: &str) -> Vec<Tok> {
        let mut toks: Vec<Tok> = lex(src).expect("lex failed").into_iter().map(|t| t.tok).collect();
        toks.pop(); // EOF
        toks
    }

    #[test]
    fn lexes_keywords_and_identifiers() {
        assert_eq!(
            kinds("let x = true;"),
            vec![
                Tok::Let,
                Tok::Ident("x".into()),
                Tok::Assign,
                Tok::True,
                Tok::Semi
            ]
        );
    }

    #[test]
    fn lexes_numbers_with_fractions() {
        assert_eq!(kinds("1 2.5 0.5"), vec![Tok::Num(1.0), Tok::Num(2.5), Tok::Num(0.5)]);
    }

    #[test]
    fn lexes_two_character_operators() {
        assert_eq!(
            kinds("== != <= >= && ||"),
            vec![
                Tok::EqEq,
                Tok::BangEq,
                Tok::LtEq,
                Tok::GtEq,
                Tok::AndAnd,
                Tok::OrOr
            ]
        );
    }

    #[test]
    fn skips_line_comments() {
        assert_eq!(kinds("1 // comment\n2"), vec![Tok::Num(1.0), Tok::Num(2.0)]);
    }

    #[test]
    fn skips_block_comments() {
        assert_eq!(kinds("1 /* two\nlines */ 2"), vec![Tok::Num(1.0), Tok::Num(2.0)]);
    }

    #[test]
    fn tracks_line_numbers() {
        let toks = lex("let a;\nlet b;\nlet c;").unwrap();
        let lines: Vec<u32> = toks.iter().map(|t| t.line).collect();
        // 3 statements x 3 tokens each, then EOF on the last line.
        assert_eq!(lines, vec![1, 1, 1, 2, 2, 2, 3, 3, 3, 3]);
    }

    #[test]
    fn counts_lines_inside_block_comments() {
        let toks = lex("/*\n\n\n*/ let x;").unwrap();
        assert_eq!(toks[0].line, 4);
    }

    #[test]
    fn rejects_unexpected_character() {
        let err = lex("let x = @;").unwrap_err();
        assert!(err.contains("unexpected character '@'"), "got: {err}");
    }

    #[test]
    fn rejects_unterminated_block_comment() {
        let err = lex("/* never closed").unwrap_err();
        assert!(err.contains("unterminated block comment"), "got: {err}");
    }

    #[test]
    fn rejects_invalid_number() {
        // "1.2.3" lexes as one run of digits and dots, which is not a number.
        let err = lex("1.2.3").unwrap_err();
        assert!(err.contains("invalid number literal"), "got: {err}");
    }
}
