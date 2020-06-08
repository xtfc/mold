use super::remote;
use failure::err_msg;
use failure::format_err;
use failure::Error;
use pest::iterators::Pair;
use pest::iterators::Pairs;
use pest::Parser;
use pest_derive::Parser;
use std::str::FromStr;

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

  /// Given a Pair, convert it into an Expr
  pub fn from(pair: Pair<Rule>) -> Self {
    use Expr::*;
    use Rule::*;

    match pair.as_rule() {
      or_expr => {
        let mut inner = pair.into_inner();
        let lhs = consume_expr(&mut inner).unwrap();
        let rhs = consume_expr(&mut inner).unwrap();
        Or(lhs.into(), rhs.into())
      }

      and_expr => {
        let mut inner = pair.into_inner();
        let lhs = consume_expr(&mut inner).unwrap();
        let rhs = consume_expr(&mut inner).unwrap();
        And(lhs.into(), rhs.into())
      }

      not_expr => Not(single_expr(pair).into()),
      atom | group => single_expr(pair),
      name => Atom(pair.as_str().into()),
      wild => Wild,
      _ => unreachable!(),
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
  Default(String, String),
  Version(String),
}

impl Statement {
  /// Given a Pair, convert it into a Statement
  pub fn from(pair: Pair<Rule>) -> Self {
    use Rule::*; // a consequence is that no variables can shadow one of these
    use Statement::*;

    match pair.as_rule() {
      if_stmt | if_recipe_stmt => {
        let mut inner = pair.into_inner();
        let cond = consume_expr(&mut inner).unwrap();
        let body = consume_statements(&mut inner);
        If(cond, body)
      }

      import_stmt => {
        let mut inner = pair.into_inner();
        let source = consume_string(&mut inner).unwrap();
        let dep_name = consume_name(&mut inner);
        Import(source, dep_name)
      }

      recipe_stmt => {
        let mut inner = pair.into_inner();
        let rec_name = consume_name(&mut inner).unwrap();
        let stmts = consume_statements(&mut inner);
        Recipe(rec_name, stmts)
      }

      var_stmt => {
        let mut inner = pair.into_inner();
        let var_name = consume_name(&mut inner).unwrap();
        let value = consume_string(&mut inner).unwrap();
        Var(var_name, value)
      }

      default_stmt => {
        let mut inner = pair.into_inner();
        let var_name = consume_name(&mut inner).unwrap();
        let value = consume_string(&mut inner).unwrap();
        Default(var_name, value)
      }

      dir_stmt => Dir(single_string(pair)),
      help_stmt => Help(single_string(pair)),
      require_stmt => Require(single_name(pair)),
      run_stmt => Run(single_string(pair)),
      version_stmt => Version(single_string(pair)),
      _ => unreachable!(),
    }
  }
}

#[derive(Parser)]
#[grammar = "mold.pest"]
struct MoldParser;

/// Given a Pairs iterator, try to yank a `string` out of it
fn consume_string(pairs: &mut Pairs<Rule>) -> Option<String> {
  pairs
    .next()
    .and_then(|x| x.into_inner().next())
    .map(|x| unescape(x.as_str()))
}

/// Given a &str, unescape special characters
///
/// This should potentially return an error, but that really complicates the
/// above API, so it just silently permits and ignores invalid escapes. Oops.
fn unescape(source: &str) -> String {
  let mut new = String::with_capacity(source.len());
  let mut chars = source.chars();

  while let Some(ch) = chars.next() {
    if ch == '\\' {
      if let Some(ch2) = chars.next() {
        new.push(match ch2 {
          'n' => '\n',
          'r' => '\r',
          't' => '\t',
          x => x,
        });
        continue;
      }
    }
    new.push(ch);
  }

  new
}

/// Given a Pairs iterator, try to yank a `name` out of it
fn consume_name(pairs: &mut Pairs<Rule>) -> Option<String> {
  pairs.next().map(|x| x.as_str().to_string())
}

/// Given a Pairs iterator, try to yank an `expr` out of it
fn consume_expr(pairs: &mut Pairs<Rule>) -> Option<Expr> {
  pairs.next().map(Expr::from)
}

/// Given a Pairs iterator, try to yank a lot of `stateent`s out of it
fn consume_statements(pairs: &mut Pairs<Rule>) -> Vec<Statement> {
  pairs
    .filter(|x| x.as_rule() != Rule::EOI)
    .map(Statement::from)
    .collect()
}

/// Given a Pair, consume a single `string` from it
fn single_string(pair: Pair<Rule>) -> String {
  consume_string(&mut pair.into_inner()).unwrap()
}

/// Given a Pair, consume a single `name` from it
fn single_name(pair: Pair<Rule>) -> String {
  consume_name(&mut pair.into_inner()).unwrap()
}

/// Given a Pair, consume a single `expr` from it
fn single_expr(pair: Pair<Rule>) -> Expr {
  consume_expr(&mut pair.into_inner()).unwrap()
}

/// Given a &str of mold lang code, convert it into a pest parse tree
fn parse(code: &str) -> Result<Vec<Statement>, Error> {
  let mut main = MoldParser::parse(Rule::main, code)?;
  let stmts = consume_statements(&mut main);
  Ok(stmts)
}

/// Given a &str of code and an EnvSet, compile it into a Moldfile
pub fn compile(code: &str, envs: &super::EnvSet) -> Result<super::Moldfile, Error> {
  use Statement::*;
  let statements = flatten(parse(code)?, envs)?;

  let mut version = None;
  let mut dir = None;
  let mut includes = super::IncludeVec::new();
  let mut recipes = super::RecipeMap::new();
  let mut vars = super::VarMap::new();

  for stmt in statements {
    match stmt {
      Version(s) => {
        if version.is_none() {
          version = Some(s);
        } else {
          return Err(format_err!("Duplicate version specified: {}", s));
        }
      }

      Help(_) => {}

      Import(url, prefix) => includes.push(super::Include {
        remote: remote::Remote::from_str(&url)?,
        prefix: prefix.unwrap_or_else(|| "".to_string()),
      }),

      Var(name, value) => {
        vars.insert(name, value);
      }

      Default(name, value) => {
        if std::env::var(&name).is_err() {
          vars.insert(name, value);
        }
      }

      Recipe(name, body) => {
        recipes.insert(name, compile_recipe(body, envs)?);
      }

      Dir(path) => {
        dir = Some(path);
      }

      Require(_) | If(_, _) | Run(_) => {
        unreachable!();
      }
    }
  }

  let version = version.ok_or_else(|| err_msg("File version must be specified"))?;

  Ok(super::Moldfile {
    version,
    includes,
    recipes,
    vars,
    dir,
  })
}

/// Given a Vec<Statement> and an EnvSet, compile it into a Recipe
pub fn compile_recipe(body: Vec<Statement>, envs: &super::EnvSet) -> Result<super::Recipe, Error> {
  use Statement::*;

  let mut help = None;
  let mut dir = None;
  let mut commands = vec![];
  let mut requires = super::TargetSet::new();
  let mut vars = super::VarMap::new();

  let body = flatten(body, envs)?;

  for stmt in body {
    match stmt {
      Help(s) => {
        help = Some(s);
      }

      Dir(s) => {
        dir = Some(s);
      }

      Var(name, value) => {
        vars.insert(name, value);
      }

      Default(name, value) => {
        if std::env::var(&name).is_err() {
          vars.insert(name, value);
        }
      }

      Run(cmd) => {
        commands.push(cmd);
      }

      Require(recipe) => {
        requires.insert(recipe);
      }

      If(_, _) | Version(_) | Import(_, _) | Recipe(_, _) => {
        unreachable!();
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

/// Given a Vec<Statement> and an EnvSet, remove all falsy If statements
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
