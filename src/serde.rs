use failure::err_msg;
use failure::Error;
use std::iter::Peekable;
use std::slice::Iter;
use std::str::Chars;

type CharIter<'a> = Peekable<Chars<'a>>;
type TokenIter<'a> = Peekable<Iter<'a, Token>>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
  And,
  As,
  Cul,
  Cur,
  Eq,
  Help,
  If,
  Import,
  Not,
  Or,
  Pal,
  Par,
  Recipe,
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
  Import(String, Option<String>),
  Var(String, String),
  If(Expr, Vec<Statement>),
  Recipe(String, Vec<Statement>),
  Help(String),
  Run(String),
}

pub fn compile(code: &str) -> Result<Statement, Error> {
  let tokens = lex(code)?;
  parse(&tokens)
}

/// Return true if the next token in `it` is `kind`
fn peek_token(it: &mut TokenIter, kind: Token) -> bool {
  if let Some(&tok) = it.peek() {
    *tok == kind
  } else {
    false
  }
}

/// Return a String if the next token in `it` is a `String`
fn use_string(it: &mut TokenIter) -> Option<String> {
  if let Some(Token::String(s)) = it.peek() {
    it.next();
    Some(s.to_string())
  } else {
    None
  }
}

/// Return a String if the next token in `it` is a `Name`
fn use_name(it: &mut TokenIter) -> Option<String> {
  if let Some(Token::Name(s)) = it.peek() {
    it.next();
    Some(s.to_string())
  } else {
    None
  }
}

/// Return true if the next token in `it` is `kind` *and* consume the token
fn use_token(it: &mut TokenIter, kind: Token) -> bool {
  if let Some(&tok) = it.peek() {
    if *tok == kind {
      it.next();
    }
    *tok == kind
  } else {
    false
  }
}

/// Return an Err if the next token in `it` is *not* `kind`
fn require_token(it: &mut TokenIter, kind: Token) -> Result<(), Error> {
  if let Some(&tok) = it.peek() {
    if *tok == kind {
      it.next();
      return Ok(());
    }

    return Err(err_msg("Oops"));
  }

  Err(err_msg("Oops"))
}

fn parse(tokens: &[Token]) -> Result<Statement, Error> {
  let mut it: TokenIter = tokens.iter().peekable();
  let mut stmts = vec![];
  while let Some(_) = it.peek() {
    let stmt = parse_stmt(&mut it)?;
    dbg!(&stmt);
    stmts.push(stmt);
  }

  Ok(Statement::File(stmts))
}

fn parse_stmt(it: &mut TokenIter) -> Result<Statement, Error> {
  match it.peek() {
    Some(Token::Version) => parse_version(it),
    Some(Token::Import) => parse_import(it),
    Some(Token::Var) => parse_var(it),
    Some(Token::If) => parse_if(it),
    Some(Token::Help) => parse_help(it),
    //Some(Token::Recipe) => parse_recipe(it),
    _ => Err(err_msg("Can't work")),
  }
}

fn parse_version(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Version
  let version = use_string(it).ok_or(err_msg("Expected version string after `version` keyword"))?;
  Ok(Statement::Version(version))
}

fn parse_help(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Help
  let desc = use_string(it).ok_or(err_msg("Expected help string after `help` keyword"))?;
  Ok(Statement::Help(desc))
}

fn parse_import(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Import

  let url = use_string(it).ok_or(err_msg("Expected URL string after `import` keyword"))?;

  let prefix = if use_token(it, Token::As) {
    Some(use_string(it).ok_or(err_msg("Expected prefix string after `as` keyword"))?)
  } else {
    None
  };

  Ok(Statement::Import(url, prefix))
}

fn parse_var(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Var

  let name = use_name(it).ok_or(err_msg("Expected variable name after `var` keyword"))?;

  if !use_token(it, Token::Eq) {
    return Err(err_msg("Expected = operator after variable name"));
  }

  let val = use_string(it).ok_or(err_msg("Expected value string after = operator"))?;

  Ok(Statement::Var(name, val))
}

fn parse_if(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::If

  let expr = parse_expr(it)?;

  if !use_token(it, Token::Cul) {
    return Err(err_msg("Expected { bracket after condition"));
  }

  let mut body = vec![];

  loop {
    if use_token(it, Token::Cur) {
      break;
    }
    body.push(parse_stmt(it)?);
  }

  Ok(Statement::If(expr, body))
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
      '"' => Some(lex_string(&mut it)?),
      '$' => Some(Token::Run),
      '=' => Some(Token::Eq),
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
    "as" => Token::As,
    "help" => Token::Help,
    "if" => Token::If,
    "import" => Token::Import,
    "recipe" => Token::Recipe,
    "run" => Token::Run,
    "var" => Token::Var,
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
          '"' => {
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
