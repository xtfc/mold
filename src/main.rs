// use colored::*;
use exitfailure::ExitFailure;
use failure::Error;
use mold::Mold;
use std::path::Path;
use std::path::PathBuf;
use structopt::StructOpt;

/// A fresh task runner
#[derive(StructOpt, Debug)]
#[structopt(author, global_settings(&[structopt::clap::AppSettings::ColoredHelp]))]
pub struct Args {
  /// Path to the moldfile
  #[structopt(long = "file", short = "f")]
  pub file: Option<PathBuf>,

  /// Comma-separated list of mold environments to activate
  #[structopt(long = "env", short = "e", env = "MOLDENV")]
  pub env: Option<String>,

  /// Single mold environment to append to list of active environments
  #[structopt(long = "add", short = "a", number_of_values = 1)]
  pub add_envs: Vec<String>,

  /// Fetch new updates for all downloaded remote data
  #[structopt(long = "update", short = "u")]
  pub update: bool,

  /// Remove all downloaded remote data
  #[structopt(long = "clean")]
  pub clean: bool,

  /// Download all remote data
  #[structopt(long = "clone")]
  pub clone: bool,

  /// Download all remote data
  #[structopt(long = "explain", short = "x")]
  pub explain: bool,

  /// Which recipe(s) to run
  pub targets: Vec<String>,
}

fn run(args: Args) -> Result<(), Error> {
  // load the moldfile
  let mut envs = vec![];
  envs.extend(args.env);
  envs.extend(args.add_envs);
  envs.push(std::env::consts::FAMILY.to_string());
  envs.push(std::env::consts::OS.to_string());

  let filepath = Mold::discover(&Path::new("."), args.file.clone())?;

  // early return if we passed a --clean
  if args.clean {
    return Mold::clean_all(&filepath);
  }

  let mold = Mold::init(&filepath, envs)?;

  // early return if we passed a --update
  if args.update {
    return mold.update_all();
  }

  // early return and print help if we didn't pass any targets
  if args.targets.is_empty() {
    return mold.help();
  }

  /*
  // explain the root moldfile if requested.
  // this is separate from the `if args.explain` below because we want this
  // to happen even if there are no arguments.
  if args.explain {
    mold.explain_self()?;
  }

  // explain all of the given targets rather than executing them
  if args.explain {
    for target_name in &args.targets {
      mold.explain(target_name)?;
    }

    return Ok(());
  }
  */

  let requested_targets = args
    .targets
    .iter()
    .map(std::string::ToString::to_string)
    .collect();
  let all_targets = mold.find_all_dependencies(&requested_targets)?;

  for target_name in &all_targets {
    mold.execute(target_name)?;
  }

  Ok(())
}

fn main() -> Result<(), ExitFailure> {
  let args = Args::from_args();
  env_logger::init();

  run(args)?;

  Ok(())
}
