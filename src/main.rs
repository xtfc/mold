use exitfailure::ExitFailure;
use failure::Error;
use mold::Mold;
use mold::TaskSet;
use std::path::PathBuf;
use structopt::StructOpt;

/// A fresh task runner
#[derive(StructOpt, Debug)]
#[structopt(raw(setting = "structopt::clap::AppSettings::ColoredHelp"))]
pub struct Args {
  /// Path to the moldfile
  #[structopt(long = "file", short = "f", default_value = "moldfile")]
  pub file: PathBuf,

  /// Don't print extraneous information
  #[structopt(long = "quiet", short = "q")]
  pub quiet: bool,

  /// dbg! the parsed moldfile
  #[structopt(long = "debug", short = "d")]
  pub debug: bool,

  /// Don't actually execute any commands
  #[structopt(long = "dry")]
  pub dry: bool,

  #[structopt(long = "update", short = "u")]
  pub update: bool,

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
  let mold = Mold::discover(&args.file)?;

  // early return if we passed a --update
  if args.update {
    return mold.update_all();
  }

  // optionally spew the parsed structure
  if args.debug {
    dbg!(&mold);
  }

  // print help if we didn't pass any targets
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

  if args.debug {
    dbg!(&targets);
  }

  // generate a Task for each target
  let mut tasks = vec![];
  for target_name in &targets {
    tasks.push(mold.find_task(&target_name, mold.env())?);
  }

  if args.debug {
    dbg!(&tasks);
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
