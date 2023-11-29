// use std::collections::HashMap as AHashMap;

use ahash::AHashMap;
use logos::Span;

use super::{
    lexer::{FailedToLexCharacter, Token, TokenStream},
    runner::environment::Function,
    Environment, Spanned,
};

pub type AST = Vec<Spanned<Expression>>;

macro_rules! expect {
    ($tokens:ident, $token:pat, $tokenexpr:expr) => {
        match $tokens.next() {
            Some(Ok($token)) => $tokenexpr,
            Some(Ok(token)) => {
                return Err(ParseError::ExpectedTokenButGot {
                    expected: $tokenexpr,
                    got: token,
                    span: $tokens.span(),
                })
            }
            Some(Err(FailedToLexCharacter)) => {
                return Err(ParseError::FailedToLexCharacter($tokens.span()))
            }
            None => return Err(ParseError::ExpectedMoreTokens($tokens.span())),
        }
    };
}

#[derive(Debug, Clone)]
pub enum Expression {
    VarAssign {
        name: Box<Spanned<Expression>>,
        value: Box<Spanned<Expression>>,
    },
    Number(f64),
    Variable(String),
    BinaryOp {
        left: Box<Spanned<Expression>>,
        operator: Operator,
        right: Box<Spanned<Expression>>,
    },
    ForLoop {
        index_name: String,
        loop_count: u64,
        block: AST,
    },
    MemberExpression {
        left: Box<Spanned<Expression>>,
        right: String,
    },
    UnaryOp(Box<Spanned<Expression>>),
    Dereference(Box<Spanned<Expression>>),
    StructObject {
        name: String,
        map: AHashMap<String, Spanned<Expression>>,
    },
    String(String),
    Borrow(Box<Spanned<Expression>>),
    None,
    Function {
        name: String,
        arguments: Vec<Spanned<Expression>>,
    },
    Object(AHashMap<String, Spanned<Expression>>),
}

#[derive(Debug, Clone)]
pub enum Operator {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug)]
pub enum ParseError {
    FailedToLexCharacter(Span),
    ExpectedMoreTokens(Span),
    ExpectedTokenButGot {
        expected: Token,
        got: Token,
        span: Span,
    },
    UnexpectedToken(Spanned<Token>),
}

pub fn parse(tokens: &mut TokenStream, environment: &Environment) -> Result<AST, ParseError> {
    let mut ast = Vec::new();
    while let Some(_) = tokens.peek() {
        ast.push(parse_expression(tokens, environment)?);
    }
    Ok(ast)
}

fn parse_expression(
    tokens: &mut TokenStream,
    environment: &Environment,
) -> Result<Spanned<Expression>, ParseError> {
    Ok(match tokens.peek() {
        Some(Ok(Token::For)) => {
            let start = tokens.span().start;
            tokens.next();

            let index_name = tokens.slice().to_string();

            tokens.next();

            match tokens.next() {
                Some(Ok(Token::In)) => {}
                _ => todo!(),
            }

            let loop_count = match parse_additive(tokens, environment)?.value {
                Expression::Number(number) => number as u64,
                t => todo!("{t:?}"),
            };

            let block = parse_block(tokens, environment)?;
            let end = tokens.span().end;

            Spanned {
                value: Expression::ForLoop {
                    index_name,
                    loop_count,
                    block,
                },
                span: start..end,
            }
        }
        Some(Ok(_)) => {
            let expr = parse_additive(tokens, environment)?;

            match tokens.peek() {
                Some(Ok(Token::Equals)) => parse_var_assign(expr, tokens, environment)?,
                _ => expr,
            }
        }
        Some(Err(FailedToLexCharacter)) => {
            return Err(ParseError::FailedToLexCharacter(tokens.peek_span()))
        }
        None => return Err(ParseError::ExpectedMoreTokens(tokens.peek_span())),
    })
}
fn parse_block(tokens: &mut TokenStream, environment: &Environment) -> Result<AST, ParseError> {
    expect!(tokens, Token::LeftBracket, Token::LeftBracket);
    let ast = parse(tokens, environment)?;
    expect!(tokens, Token::RightBracket, Token::RightBracket);
    Ok(ast)
}
fn parse_additive(
    tokens: &mut TokenStream,
    environment: &Environment,
) -> Result<Spanned<Expression>, ParseError> {
    let mut node = parse_multiplicitive(tokens, environment)?;

    while let Some(Ok(Token::Plus | Token::Minus)) = tokens.peek() {
        let operator = match tokens.next() {
            Some(Ok(Token::Plus)) => Operator::Add,
            Some(Ok(Token::Minus)) => Operator::Sub,
            _ => unreachable!(),
        };

        let right = parse_multiplicitive(tokens, environment)?;

        node = Spanned {
            span: node.span.start..right.span.end,
            value: Expression::BinaryOp {
                left: Box::new(node),
                operator,
                right: Box::new(right),
            },
        };
    }

    Ok(node)
}
fn parse_multiplicitive(
    tokens: &mut TokenStream,
    environment: &Environment,
) -> Result<Spanned<Expression>, ParseError> {
    let mut node = parse_primary(tokens, environment)?;

    while let Some(Ok(Token::Asterisk | Token::Slash | Token::Modulo)) = tokens.peek() {
        let operator = match tokens.next() {
            Some(Ok(Token::Asterisk)) => Operator::Mul,
            Some(Ok(Token::Slash)) => Operator::Div,
            Some(Ok(Token::Modulo)) => Operator::Mod,
            _ => unreachable!(),
        };

        let right = parse_primary(tokens, environment)?;

        node = Spanned {
            span: node.span.start..right.span.end,
            value: Expression::BinaryOp {
                left: Box::new(node),
                operator,
                right: Box::new(right),
            },
        };
    }

    Ok(node)
}
fn parse_primary(
    tokens: &mut TokenStream,
    environment: &Environment,
) -> Result<Spanned<Expression>, ParseError> {
    let mut expr = match tokens.next() {
        Some(Ok(Token::LeftParen)) => {
            if let Some(Ok(Token::RightParen)) = tokens.peek() {
                let start = tokens.span().start;
                tokens.next();
                Ok(Spanned {
                    span: start..tokens.span().end,
                    value: Expression::None,
                })
            } else {
                let expr = parse_expression(tokens, environment)?;
                expect!(tokens, Token::RightParen, Token::RightParen);
                Ok(expr)
            }
        }
        Some(Ok(Token::Identifer)) => match tokens.peek() {
            Some(Ok(Token::LeftBracket)) => {
                let name = tokens.slice().to_string();

                expect!(tokens, Token::LeftBracket, Token::LeftBracket);
                let map = parse_object(tokens, environment)?;

                Ok(Spanned {
                    span: tokens.span(),
                    value: Expression::StructObject { name, map },
                })
            }
            _ => {
                if let Some(Function { argument_count, .. }) =
                    environment.get_function(tokens.slice())
                {
                    let name = tokens.slice().to_string();
                    let start = tokens.span().start;
                    let mut arguments = Vec::new();
                    for _ in 0..(*argument_count) {
                        let expr = parse_expression(tokens, environment)?;
                        arguments.push(expr);
                    }
                    Ok(Spanned {
                        span: start..tokens.span().end,
                        value: Expression::Function { name, arguments },
                    })
                } else {
                    Ok(tokens.wrap_span(Expression::Variable(tokens.slice().to_string())))
                }
            }
        },
        Some(Ok(Token::LeftBracket)) => {
            let map = parse_object(tokens, environment)?;

            Ok(Spanned {
                span: tokens.span(),
                value: Expression::Object(map),
            })
        }
        Some(Ok(Token::String)) => {
            let slice = tokens.slice();
            let string = slice[1..slice.len() - 1].to_string();
            Ok(tokens.wrap_span(Expression::String(string)))
        }
        Some(Ok(Token::Minus)) => {
            let expr = parse_primary(tokens, environment)?;
            Ok(tokens.wrap_span(Expression::UnaryOp(Box::new(expr))))
        }
        Some(Ok(Token::Ampersand)) => {
            let expr = parse_primary(tokens, environment)?;
            Ok(tokens.wrap_span(Expression::Borrow(Box::new(expr))))
        }
        Some(Ok(Token::Asterisk)) => {
            let expr = parse_primary(tokens, environment)?;
            Ok(tokens.wrap_span(Expression::Dereference(Box::new(expr))))
        }
        Some(Ok(Token::Number)) => {
            Ok(tokens.wrap_span(Expression::Number(tokens.slice().parse().unwrap())))
        }
        Some(Ok(token)) => Err(ParseError::UnexpectedToken(tokens.wrap_span(token))),
        Some(Err(FailedToLexCharacter)) => Err(ParseError::FailedToLexCharacter(tokens.span())),
        None => unreachable!("oh fuck what have i done to cause this to happen"),
    }?;
    // If theres a dot after the expression, do a member expression:
    while let Some(Ok(Token::Dot)) = tokens.peek() {
        tokens.next(); // skip the dot
        expect!(tokens, Token::Identifer, Token::Identifer);
        let right = tokens.slice().to_string();
        expr = Spanned {
            span: expr.span.start..tokens.span().end,
            value: Expression::MemberExpression {
                left: Box::new(expr),
                right,
            },
        };
    }
    Ok(expr)
}

fn parse_var_assign(
    name: Spanned<Expression>,
    tokens: &mut TokenStream<'_>,
    environment: &Environment,
) -> Result<Spanned<Expression>, ParseError> {
    tokens.next(); // We already know that the next token is an equals

    let value = parse_additive(tokens, environment)?;

    Ok(Spanned {
        span: name.span.start..value.span.end,
        value: Expression::VarAssign {
            name: Box::new(name),
            value: Box::new(value),
        },
    })
}

/// Parses an object.
///
/// - `{}`
/// - `{test: 4}`
/// - `{str: "sup!", num: -6.2}`
fn parse_object(
    tokens: &mut TokenStream,
    environment: &Environment,
) -> Result<AHashMap<String, Spanned<Expression>>, ParseError> {
    let mut map = AHashMap::new();
    while let Some(Ok(Token::Identifer)) = tokens.peek() {
        tokens.next();
        let ident = tokens.slice().to_string();
        expect!(tokens, Token::Colon, Token::Colon);
        let expr = parse_expression(tokens, environment)?;
        map.insert(ident, expr);
        match tokens.peek() {
            Some(Ok(Token::RightBracket)) => break,
            Some(Ok(Token::Comma)) => {
                tokens.next();
            }
            token => todo!("{token:?}"),
        }
    }
    expect!(tokens, Token::RightBracket, Token::RightBracket);
    Ok(map)
}

#[cfg(test)]
mod tests {
    use crate::parser::Environment;

    use super::{super::lexer::TokenStream, parse};

    #[test]
    fn var_assign() {
        let mut lexer = TokenStream::new("x = 1 + 2 - 30 + y");
        let environment = Environment::default();

        let ast = parse(&mut lexer, &environment);

        assert!(ast.is_ok());

        // TODO: figure out how to assert ast
    }
}
