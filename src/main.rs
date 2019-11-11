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

  /// Which recipe(s) to run
  pub targets: Vec<String>,
}

fn run(args: Args) -> Result<(), Error> {
  // load the moldfile
  let mut mold = Mold::discover(&Path::new("."), args.file.clone())?;
  mold.set_envs(args.env);
  mold.add_envs(args.add_envs);

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

  // early return and print help if we didn't pass any targets
  if args.targets.is_empty() {
    return mold.help();
  }

  let targets = args
    .targets
    .iter()
    .map(std::string::ToString::to_string)
    .collect();
  let deps = mold.find_all_dependencies(&targets)?;
  dbg!(deps);

  Ok(())
}

fn main() -> Result<(), ExitFailure> {
  let args = Args::from_args();
  env_logger::init();

  run(args)?;

  Ok(())
}
