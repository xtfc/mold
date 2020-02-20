use failure::err_msg;
use failure::Error;
use std::iter::Peekable;
use std::slice::Iter;
use std::str::Chars;

type CharIter<'a> = Peekable<Chars<'a>>;
type TokenIter<'a> = Peekable<Iter<'a, Token>>;

#[derive(Debug, Clone)]
enum Token {
  And,
  As,
  Cul,
  Cur,
  Help,
  If,
  Import,
  Not,
  Or,
  Pal,
  Par,
  Run,
  Sql,
  Sqr,
  Var,
  Version,
  Wild,

  Name(String),
  String(String),
}

#[derive(Debug, Clone)]
pub enum Expr {
  And(Box<Expr>, Box<Expr>),
  Or(Box<Expr>, Box<Expr>),
  Not(Box<Expr>),
  Group(Box<Expr>),
  Atom(String),
  Wild,
}


#[derive(Debug, Clone)]
pub enum Statement {
  File(Vec<Statement>),
  Version(String),
  Import(String, String),
  Var(String, Box<Expr>),
  If(Box<Expr>, Vec<Statement>),
  Help(String),
  Run(String),
}

pub fn compile(code: &str) -> Result<Statement, Error> {
  let tokens = lex(code)?;
  parse(&tokens)
}

fn parse(tokens: &[Token]) -> Result<Statement, Error> {
  let mut it: TokenIter = tokens.iter().peekable();
  let stmt = parse_stmt(&mut it)?;
  dbg!(&stmt);
  Ok(stmt)
}

fn parse_stmt(it: &mut TokenIter) -> Result<Statement, Error> {
  match it.peek() {
    Some(Token::Version) => parse_version(it),
    _ => Err(err_msg("Can't work")),
  }
}

fn parse_version(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next();
  if let Some(Token::String(s)) = it.peek() {
    Ok(Statement::Version(s.to_string()))
  } else {
    Err(err_msg("Expected string after `version` keyword"))
  }
}

fn parse_expr(it: &mut TokenIter) -> Result<Expr, Error> {
  parse_or(it)
}

fn parse_or(it: &mut TokenIter) -> Result<Expr, Error> {
  let lhs = parse_and(it)?;

  if let Some(Token::Or) = it.peek() {
    it.next();
    let rhs = parse_expr(it)?;
    Ok(Expr::Or(lhs.into(), rhs.into()))
  } else {
    Ok(lhs)
  }
}

fn parse_and(it: &mut TokenIter) -> Result<Expr, Error> {
  let lhs = parse_not(it)?;

  if let Some(Token::And) = it.peek() {
    it.next();
    let rhs = parse_expr(it)?;
    Ok(Expr::And(lhs.into(), rhs.into()))
  } else {
    Ok(lhs)
  }
}

fn parse_not(it: &mut TokenIter) -> Result<Expr, Error> {
  if let Some(Token::Not) = it.peek() {
    it.next();
    let inner = parse_atom(it)?;
    Ok(Expr::Not(inner.into()))
  } else {
    parse_atom(it)
  }
}

fn parse_atom(it: &mut TokenIter) -> Result<Expr, Error> {
  match it.next() {
    Some(Token::Pal) => {
      let inner = parse_expr(it)?;
      if let Some(Token::Par) = it.next() {
        Ok(Expr::Group(inner.into()))
      } else {
        Err(err_msg("Parse error; expected close parenthesis"))
      }
    }
    Some(Token::Name(x)) => Ok(Expr::Atom(x.clone())),
    Some(Token::Wild) => Ok(Expr::Wild),
    Some(_) => Err(err_msg("Parse error; expected name or open parenthesis")),
    None => Err(err_msg("Parse error; unexpected end of expression")),
  }
}

fn lex(expr: &str) -> Result<Vec<Token>, Error> {
  let mut tokens = vec![];
  let mut it: CharIter = expr.chars().peekable();

  while let Some(c) = it.next() {
    let x = match c {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => Some(lex_name(c, &mut it)),
      '\'' => Some(lex_string(&mut it)?),
      '$' => Some(Token::Run),
      '+' => Some(Token::And),
      '|' => Some(Token::Or),
      '*' | '?' => Some(Token::Wild),
      '~' => Some(Token::Not),
      '(' => Some(Token::Pal),
      ')' => Some(Token::Par),
      '[' => Some(Token::Sql),
      ']' => Some(Token::Sqr),
      '{' => Some(Token::Cul),
      '}' => Some(Token::Cur),
      ' ' | '\t' | '\n' => None,

      _ => None,
    };
    if let Some(token) = x {
      tokens.push(token);
    }
  }

  Ok(tokens)
}

fn lex_name(first: char, it: &mut CharIter) -> Token {
  let mut name = String::new();
  name.push(first);

  while let Some(&c) = it.peek() {
    match c {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => {
        it.next();
        name.push(c);
      }
      _ => break,
    }
  }

  match name.as_str() {
    "if" => Token::If,
    "import" => Token::Import,
    "as" => Token::As,
    "var" => Token::Var,
    "run" => Token::Run,
    "help" => Token::Help,
    "version" => Token::Version,
    _ => Token::Name(name),
  }
}

fn lex_string(it: &mut CharIter) -> Result<Token, Error> {
  let mut contents = String::new();
  let mut escaped = false;

  loop {
    if let Some(c) = it.next() {
      if escaped {
        match c {
          'n' => {
            contents.push('\n');
          }
          'r' => {
            contents.push('\r');
          }
          't' => {
            contents.push('\t');
          }
          _ => {
            contents.push(c.clone());
          }
        }
        escaped = false;
      } else {
        match c {
          '\'' => {
            break;
          }
          '\\' => {
            escaped = true;
          }
          _ => {
            contents.push(c.clone());
          }
        }
      }
    } else {
      return Err(err_msg("Unterminated string"));
    }
  }

  Ok(Token::String(contents))
}
