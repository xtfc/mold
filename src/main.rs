use exitfailure::ExitFailure;
use failure::Error;
use mold::Mold;
use mold::TaskSet;
use std::path::Path;
use std::path::PathBuf;
use structopt::StructOpt;

/// A fresh task runner
#[derive(StructOpt, Debug)]
#[structopt(raw(setting = "structopt::clap::AppSettings::ColoredHelp"))]
pub struct Args {
  /// Path to the moldfile
  #[structopt(long = "file", short = "f")]
  pub file: Option<PathBuf>,

  /// Print the parsed moldfile before processing it
  #[structopt(long = "debug", short = "d")]
  pub debug: bool,

  /// Don't actually execute any commands
  #[structopt(long = "dry")]
  pub dry: bool,

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

fn main() -> Result<(), ExitFailure> {
  let args = Args::from_args();
  env_logger::init();

  run(args)?;

  Ok(())
}

fn run(args: Args) -> Result<(), Error> {
  // load the moldfile
  let mut mold = Mold::discover(&Path::new("."), args.file.clone())?;

  // early return if we passed a --clean
  if args.clean {
    return mold.clean_all();
  }

  // early return if we passed a --debug
  if args.debug {
    dbg!(&mold);
    return Ok(());
  }

  // we'll actually be doing something if we get this far, so we want to make
  // sure we have all of the Groups and Includes cloned before proceeding
  mold.clone_all()?;

  // merge all Includes
  mold.process_includes()?;

  // early return if we passed a --clone
  if args.clone {
    return Ok(());
  }

  // early return if we passed a --update
  if args.update {
    return mold.update_all();
  }

  // early return and print help if we didn't pass any targets
  if args.targets.is_empty() {
    return mold.help();
  }

  // find all recipes to run, including all dependencies
  let targets_set: TaskSet = args
    .targets
    .iter()
    .map(std::string::ToString::to_string)
    .collect();
  let targets = mold.find_all_dependencies(&targets_set)?;

  // generate a Task for each target
  let mut tasks = vec![];
  for target_name in &targets {
    if let Some(task) = mold.find_task(&target_name, mold.env())? {
      tasks.push(task);
    }
  }

  // execute the collected Tasks
  for task in &tasks {
    task.print_cmd();
    if args.dry {
      task.print_env();
    } else {
      task.exec()?;
    }
  }

  Ok(())
}
