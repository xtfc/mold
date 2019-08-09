use std::iter::Enumerate;
use std::iter::Peekable;
use std::str::Chars;

type CharIter<'a> = Peekable<Enumerate<Chars<'a>>>;

#[derive(Debug, Clone)]
enum Token {
  And,
  Or,
  Not,
  Pal,
  Par,
  Name(String),
}

#[derive(Debug, Clone)]
pub enum Expr {
  And(Box<Expr>, Box<Expr>),
  Or(Box<Expr>, Box<Expr>),
  Not(Box<Expr>),
  Group(Box<Expr>),
  Val(String),
}

impl Expr {
  pub fn apply(to: &[&str]) -> bool {
    false
  }
}

fn lex_name(it: &mut CharIter) -> Token {
  let mut name = String::new();
  name.push(it.next().unwrap().1);

  while let Some(&(_i, c)) = it.peek() {
    match c {
      'a'...'z' | 'A'...'Z' | '0'...'9' | '_' | '-' => {
        it.next();
        name.push(c);
      }
      _ => break,
    }
  }

  Token::Name(name)
}

fn lex(expr: &str) -> Vec<Token> {
  let mut tokens = vec![];
  let mut it: CharIter = expr.chars().enumerate().peekable();

  while let Some(&(_i, c)) = it.peek() {
    let x = match c {
      'a'...'z' | 'A'...'Z' | '0'...'9' | '_' | '-' => Some(lex_name(&mut it)),
      '+' => {
        it.next();
        Some(Token::And)
      }
      '|' => {
        it.next();
        Some(Token::Or)
      }
      '~' => {
        it.next();
        Some(Token::Not)
      }
      '(' => {
        it.next();
        Some(Token::Pal)
      }
      ')' => {
        it.next();
        Some(Token::Par)
      }
      ' ' | '\t' | '\n' => {
        it.next();
        None
      }
      _ => {
        it.next();
        None
      }
    };
    if let Some(token) = x {
      tokens.push(token);
    }
  }

  tokens
}

fn parse() {}

pub fn compile(expr: &str) -> Expr {
  dbg!(lex(expr));
  Expr::Val("foo".into())
}
