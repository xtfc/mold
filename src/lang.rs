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

#[derive(Parser)]
#[grammar = "mold.pest"]
struct MoldParser;

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
    Rule::help_stmt => Statement::Help(consume_string(&mut pair.into_inner()).unwrap()),
    Rule::require_stmt => Statement::Require(consume_name(&mut pair.into_inner()).unwrap()),

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
      let body = consume_statements(&mut inner);
      Statement::If(expr, body)
    }

    Rule::if_recipe_stmt => {
      let mut inner = pair.into_inner();
      let expr = consume_expr(&mut inner).unwrap();
      let body = consume_statements(&mut inner);
      Statement::If(expr, body)
    }

    Rule::recipe_stmt => {
      let mut inner = pair.into_inner();
      let name = consume_name(&mut inner).unwrap();
      let stmts = consume_statements(&mut inner);
      Statement::Recipe(name, stmts)
    }

    Rule::EOI => unreachable!(),

    x => {
      panic!(format!("Unknown statement rule {:?}", x));
    }
  }
}

fn convert_expr(pair: Pair<Rule>) -> Expr {
  match pair.as_rule() {
    Rule::or_expr => {
      let mut inner = pair.into_inner();
      let lhs = consume_expr(&mut inner).unwrap();
      let rhs = consume_expr(&mut inner).unwrap();
      Expr::Or(lhs.into(), rhs.into())
    }
    Rule::and_expr => {
      let mut inner = pair.into_inner();
      let lhs = consume_expr(&mut inner).unwrap();
      let rhs = consume_expr(&mut inner).unwrap();
      Expr::And(lhs.into(), rhs.into())
    }
    Rule::not_expr => Expr::Not(consume_expr(&mut pair.into_inner()).unwrap().into()),
    Rule::atom | Rule::group => consume_expr(&mut pair.into_inner()).unwrap(),
    Rule::name => Expr::Atom(pair.as_str().into()),
    Rule::wild => Expr::Wild,
    x => {
      panic!(format!("Unknown expression rule {:?}", x));
    }
  }
}

fn consume_statements(pairs: &mut Pairs<Rule>) -> Vec<Statement> {
  pairs
    .filter(|x| x.as_rule() != Rule::EOI)
    .map(convert_statement)
    .collect()
}

fn consume_expr(pairs: &mut Pairs<Rule>) -> Option<Expr> {
  pairs.next().map(convert_expr)
}

fn parse_pest(code: &str) -> Result<Vec<Statement>, Error> {
  let mut main = MoldParser::parse(Rule::main, code)?;
  let stmts = consume_statements(&mut main);
  Ok(stmts)
}

pub fn compile(code: &str, envs: &super::EnvSet) -> Result<super::Moldfile, Error> {
  let statements = flatten(parse_pest(code)?, envs)?;

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
