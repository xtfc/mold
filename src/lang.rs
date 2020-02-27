use super::remote;
use failure::err_msg;
use failure::format_err;
use failure::Error;
use std::iter::Peekable;
use std::slice::Iter;
use std::str::Chars;
use std::str::FromStr;

type CharIter<'a> = Peekable<Chars<'a>>;
type TokenIter<'a> = Peekable<Iter<'a, Token>>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
  And,
  As,
  Cul,
  Cur,
  Dir,
  Eq,
  Help,
  If,
  Import,
  Not,
  Or,
  Pal,
  Par,
  Recipe,
  Require,
  Run,
  Sql,
  Sqr,
  Var,
  Version,
  Wild,

  Name(String),
  String(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
  And(Box<Expr>, Box<Expr>),
  Or(Box<Expr>, Box<Expr>),
  Not(Box<Expr>),
  Group(Box<Expr>),
  Atom(String),
  Wild,
}

impl Expr {
  pub fn apply(&self, to: &super::EnvSet) -> bool {
    match self {
      Expr::And(x, y) => x.apply(to) && y.apply(to),
      Expr::Or(x, y) => x.apply(to) || y.apply(to),
      Expr::Not(x) => !x.apply(to),
      Expr::Group(x) => x.apply(to),
      Expr::Atom(x) => to.contains(x),
      Expr::Wild => true,
    }
  }
}

// FIXME inline scripts?
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Statement {
  Dir(String),
  Help(String),
  If(Expr, Vec<Statement>),
  Import(String, Option<String>),
  Recipe(String, Vec<Statement>),
  Require(String),
  Run(String),
  Var(String, String),
  Version(String),
}

// use pest::Parser;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "mold.pest"]
struct MoldParser;

use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::Parser;

fn consume_string(pairs: &mut Pairs<Rule>) -> Option<String> {
  pairs
    .next()
    .and_then(|x| x.into_inner().next())
    .map(|x| x.as_str().to_string())
}

fn consume_name(pairs: &mut Pairs<Rule>) -> Option<String> {
  pairs.next().map(|x| x.as_str().to_string())
}

fn convert_statement(pair: Pair<Rule>) -> Statement {
  match pair.as_rule() {
    Rule::version_stmt => Statement::Version(consume_string(&mut pair.into_inner()).unwrap()),

    Rule::dir_stmt => Statement::Dir(consume_string(&mut pair.into_inner()).unwrap()),

    Rule::import_stmt => {
      let mut inner = pair.into_inner();
      let source = consume_string(&mut inner).unwrap();
      let name = consume_name(&mut inner);
      Statement::Import(source, name)
    }

    Rule::var_stmt => {
      let mut inner = pair.into_inner();
      let name = consume_name(&mut inner).unwrap();
      let value = consume_string(&mut inner).unwrap();
      Statement::Var(name, value)
    }

    Rule::run_stmt => Statement::Run(consume_string(&mut pair.into_inner()).unwrap()),

    Rule::if_stmt => {
      let mut inner = pair.into_inner();
      let expr = consume_expr(&mut inner).unwrap();
      let body = consume_statements(&mut inner).unwrap();
      Statement::If(expr, body)
    }

    x => {
      dbg!(&pair);
      panic!(format!("found {:?}", x));
    }
  }
}

fn convert_expr(pair: Pair<Rule>) -> Expr {
  Expr::Wild
}

fn consume_statements(pairs: &mut Pairs<Rule>) -> Option<Vec<Statement>> {
  pairs
    .next()
    .map(|x| x.into_inner().map(convert_statement).collect())
}

fn consume_expr(pairs: &mut Pairs<Rule>) -> Option<Expr> {
  pairs.next().map(convert_expr)
}

fn parse_pest(code: &str) -> Result<Vec<Statement>, Error> {
  let main = MoldParser::parse(Rule::main, code)?.next().unwrap();
  let stmts = consume_statements(&mut main.into_inner()).unwrap();
  dbg!(&stmts);
  Ok(stmts)
}

pub fn compile(code: &str, envs: &super::EnvSet) -> Result<super::Moldfile, Error> {
  let tokens = lex(code)?;
  let statements = flatten(parse(&tokens)?, envs)?;
  let pest_statements = flatten(parse_pest(code)?, envs)?;

  assert!(statements == pest_statements);

  let mut version = None;
  let mut includes = super::IncludeVec::new();
  let mut recipes = super::RecipeMap::new();
  let mut vars = super::VarMap::new();

  for stmt in statements {
    match stmt {
      Statement::Version(s) => {
        if version.is_none() {
          version = Some(s);
        } else {
          return Err(format_err!("Duplicate version specified: {}", s));
        }
      }

      Statement::Help(_) => {}

      Statement::Import(url, prefix) => includes.push(super::Include {
        remote: remote::Remote::from_str(&url)?,
        prefix: prefix.unwrap_or_else(|| "".to_string()),
      }),

      Statement::Var(name, value) => {
        vars.insert(name, value);
      }

      Statement::Recipe(name, body) => {
        recipes.insert(name, compile_recipe(body, envs)?);
      }

      Statement::Require(_) | Statement::Dir(_) | Statement::If(_, _) | Statement::Run(_) => {
        return Err(err_msg("Something terrible has happened."));
      }
    }
  }

  let version = version.ok_or_else(|| err_msg("File version must be specified"))?;

  Ok(super::Moldfile {
    version,
    includes,
    recipes,
    vars,
  })
}

pub fn compile_recipe(body: Vec<Statement>, envs: &super::EnvSet) -> Result<super::Recipe, Error> {
  let mut help = None;
  let mut dir = None;
  let mut commands = vec![];
  let mut requires = super::TargetSet::new();
  let mut vars = super::VarMap::new();

  let body = flatten(body, envs)?;

  for stmt in body {
    match stmt {
      Statement::Help(s) => {
        help = Some(s);
      }

      Statement::Dir(s) => {
        dir = Some(s);
      }

      Statement::Var(name, value) => {
        vars.insert(name, value);
      }

      Statement::Run(cmd) => {
        commands.push(cmd);
      }

      Statement::Require(recipe) => {
        requires.insert(recipe);
      }

      Statement::If(_, _)
      | Statement::Version(_)
      | Statement::Import(_, _)
      | Statement::Recipe(_, _) => {
        return Err(err_msg("Something terrible has happened."));
      }
    }
  }

  Ok(super::Recipe {
    help,
    commands,
    vars,
    dir,
    requires,
  })
}

pub fn flatten(body: Vec<Statement>, envs: &super::EnvSet) -> Result<Vec<Statement>, Error> {
  let mut ret = vec![];

  for stmt in body {
    match stmt {
      Statement::If(expr, body) => {
        if expr.apply(envs) {
          ret.extend(flatten(body, envs)?);
        }
      }
      x => ret.push(x),
    }
  }

  Ok(ret)
}

pub fn compile_expr(expr: &str) -> Result<Expr, Error> {
  let tokens = lex(expr)?;
  let mut it: TokenIter = tokens.iter().peekable();
  let expr = parse_expr(&mut it)?;
  match it.next() {
    Some(_) => Err(err_msg("Parse error; expected end of expression")),
    None => Ok(expr),
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

fn parse(tokens: &[Token]) -> Result<Vec<Statement>, Error> {
  let mut it: TokenIter = tokens.iter().peekable();
  let mut stmts = vec![];
  while let Some(_) = it.peek() {
    let stmt = parse_stmt(&mut it)?;
    stmts.push(stmt);
  }

  Ok(stmts)
}

fn parse_stmt(it: &mut TokenIter) -> Result<Statement, Error> {
  match it.peek() {
    Some(Token::Version) => parse_version(it),
    Some(Token::Import) => parse_import(it),
    Some(Token::Var) => parse_var(it),
    Some(Token::If) => parse_if(it, parse_stmt),
    Some(Token::Help) => parse_help(it),
    Some(Token::Recipe) => parse_recipe(it),
    Some(x) => Err(failure::format_err!(
      "Unexpected token {:?} when parsing top-level statements",
      x
    )),
    None => Err(err_msg(
      "Unexpected end of input while parsing top-level statements",
    )),
  }
}

fn parse_version(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Version
  let version =
    use_string(it).ok_or_else(|| err_msg("Expected version string after `version` keyword"))?;
  Ok(Statement::Version(version))
}

fn parse_help(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Help
  let desc = use_string(it).ok_or_else(|| err_msg("Expected help string after `help` keyword"))?;
  Ok(Statement::Help(desc))
}

fn parse_dir(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Dir
  let dir = use_string(it).ok_or_else(|| err_msg("Expected directory after `dir` keyword"))?;
  Ok(Statement::Dir(dir))
}

fn parse_require(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Require
  let recipe =
    use_name(it).ok_or_else(|| err_msg("Expected recipe name after `require` keyword"))?;
  Ok(Statement::Require(recipe))
}

fn parse_import(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Import

  let url = use_string(it).ok_or_else(|| err_msg("Expected URL string after `import` keyword"))?;

  let prefix = if use_token(it, Token::As) {
    Some(use_name(it).ok_or_else(|| err_msg("Expected prefix string after `as` keyword"))?)
  } else {
    None
  };

  Ok(Statement::Import(url, prefix))
}

fn parse_var(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Var

  let name = use_name(it).ok_or_else(|| err_msg("Expected variable name after `var` keyword"))?;

  if !use_token(it, Token::Eq) {
    return Err(err_msg("Expected = operator after variable name"));
  }

  let val = use_string(it).ok_or_else(|| err_msg("Expected value string after = operator"))?;

  Ok(Statement::Var(name, val))
}

fn parse_if<F>(it: &mut TokenIter, parser: F) -> Result<Statement, Error>
where
  F: Fn(&mut TokenIter) -> Result<Statement, Error>,
{
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
    body.push(parser(it)?);
  }

  Ok(Statement::If(expr, body))
}

fn parse_recipe(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Recipe

  let name = use_name(it).ok_or_else(|| err_msg("Expected name after `recipe` keyword"))?;

  if !use_token(it, Token::Cul) {
    return Err(err_msg("Expected { bracket after recipe name"));
  }

  let mut body = vec![];

  loop {
    if use_token(it, Token::Cur) {
      break;
    }
    body.push(parse_recipe_stmt(it)?);
  }

  Ok(Statement::Recipe(name, body))
}

fn parse_recipe_stmt(it: &mut TokenIter) -> Result<Statement, Error> {
  match it.peek() {
    Some(Token::Var) => parse_var(it),
    Some(Token::Help) => parse_help(it),
    Some(Token::If) => parse_if(it, parse_recipe_stmt),
    Some(Token::Run) => parse_run(it),
    Some(Token::Dir) => parse_dir(it),
    Some(Token::Require) => parse_require(it),
    Some(x) => Err(format_err!(
      "Unexpected token {:?} when parsing recipe body",
      x
    )),
    None => Err(err_msg("Unexpected end of input while parsing recipe body")),
  }
}

fn parse_run(it: &mut TokenIter) -> Result<Statement, Error> {
  it.next(); // skip Token::Run
  let cmd = use_string(it).ok_or_else(|| err_msg("Expected command string after `run` keyword"))?;
  Ok(Statement::Run(cmd))
}

// expressions

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

// lexer

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
      'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '/' | ':' => {
        it.next();
        name.push(c);
      }
      _ => break,
    }
  }

  match name.as_str() {
    "as" => Token::As,
    "dir" => Token::Dir,
    "help" => Token::Help,
    "if" => Token::If,
    "import" => Token::Import,
    "recipe" => Token::Recipe,
    "require" => Token::Require,
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
