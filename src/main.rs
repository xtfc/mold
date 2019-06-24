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

  /// Fetch new updates for all downloaded groups
  #[structopt(long = "update", short = "u")]
  pub update: bool,

  /// Remove all downloaded groups
  #[structopt(long = "clean")]
  pub clean: bool,

  /// Download all top-level groups
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
  let mut mold = match &args.file {
    Some(file) => Mold::discover(file),
    None => Mold::discover_dir(&Path::new(".")),
  }?;

  // early return if we passed a --clean
  if args.clean {
    return mold.clean_all();
  }

  // early return if we passed a --clone
  if args.clone {
    return mold.clone_all();
  }

  // early return if we passed a --update
  if args.update {
    return mold.update_all();
  }

  // early return if we passed a --debug
  if args.debug {
    dbg!(&mold);
    return Ok(());
  }

  // early return and print help if we didn't pass any targets
  if args.targets.is_empty() {
    return mold.help();
  }

  // process all includes
  mold.process_includes()?;

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
