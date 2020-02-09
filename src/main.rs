use colored::*;
use exitfailure::ExitFailure;
use failure::Error;
use mold::file::TargetSet;
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
  let mut mold = Mold::discover(&Path::new("."), args.file.clone())?;
  mold.set_envs(args.env);
  mold.add_envs(args.add_envs);

  mold.add_env(std::env::consts::FAMILY);
  mold.add_env(std::env::consts::OS);

  // early return if we passed a --clean
  if args.clean {
    return mold.clean_all();
  }

  // clone all Modules and Includes before proceeding
  mold.clone_all()?;

  // merge all Includes
  mold.process_includes()?;

  // early return if we passed a --update
  if args.update {
    return mold.update_all();
  }

  // explain the root moldfile if requested.
  // this is separate from the `if args.explain` below because we want this
  // to happen even if there are no arguments.
  if args.explain {
    mold.explain_self()?;
  }

  // early return and print help if we didn't pass any targets
  if args.targets.is_empty() {
    return mold.help();
  }

  // explain all of the given targets rather than executing them
  if args.explain {
    for target_name in &args.targets {
      mold.explain(target_name)?;
    }

    return Ok(());
  }

  let mut requested_targets = TargetSet::new();
  for target in args.targets {
    // FIXME once stabilized, use `.strip_prefix` instead. way cleaner.
    // if let Some(env) = target.strip_prefix('+') {
    if target.starts_with('+') {
      let env = target.trim_start_matches('+');
      mold.add_env(env);
    } else {
      requested_targets.insert(target.to_string());
    }
  }

  let all_targets = mold.find_all_dependencies(&requested_targets)?;
  for target_name in &all_targets {
    let args = mold.build_args(target_name)?;
    println!(
      "{} {} {} {}",
      "mold".white(),
      target_name.cyan(),
      "$".green(),
      shell_words::join(task.args()),
    );

    task.execute()?;
  }

  Ok(())
}

fn main() -> Result<(), ExitFailure> {
  let args = Args::from_args();
  env_logger::init();

  run(args)?;

  Ok(())
}
