use exitfailure::ExitFailure;
use failure::Error;
use mold::remote;
use mold::EnvMap;
use mold::Mold;
use mold::Recipe;
use mold::Task;
use std::path::PathBuf;
use structopt::StructOpt;

type TaskSet = indexmap::IndexSet<String>;

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
    return update_all(&mold);
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
  let targets_set: TaskSet = args.targets.iter().map(|x| x.to_string()).collect();
  let targets = find_all_dependencies(&mold, &targets_set)?;

  if args.debug {
    dbg!(&targets);
  }

  // generate a Task for each target
  let mut tasks = vec![];
  for target_name in &targets {
    tasks.push(find_task(
      &mold,
      &target_name,
      mold.env(),
    )?);
  }

  if args.debug {
    dbg!(&tasks);
  }

  // execute the collected Tasks
  for task in &tasks {
    if args.dry {
      task.dry();
    } else {
      task.exec()?;
    }
  }

  Ok(())
}

/// Recursively fetch/checkout for all groups that have already been cloned
fn update_all(mold: &Mold) -> Result<(), Error> {
  // find all groups that have already been cloned and update them.
  for (name, recipe) in &mold.data().recipes {
    if let Recipe::Group(group) = recipe {
      let mut path = mold.dir().clone();
      path.push(name);

      // only update groups that have already been cloned
      if path.is_dir() {
        remote::checkout(&path, &group.ref_)?;

        // recursively update subgroups
        let group = mold.open_group(name)?;
        update_all(&group)?;
      }
    }
  }

  Ok(())
}

/// Lazily clone groups for a given target
fn clone(mold: &Mold, target: &str) -> Result<(), Error> {
  // if this isn't a nested subrecipe, we don't need to worry about cloning anything
  if !target.contains('/') {
    return Ok(());
  }

  let splits: Vec<_> = target.splitn(2, '/').collect();
  let group_name = splits[0];
  let recipe_name = splits[1];

  let recipe = mold.find_group(group_name)?;
  let mut path = mold.dir().clone();
  path.push(group_name);

  // if the directory doesn't exist, we need to clone it
  if !path.is_dir() {
    remote::clone(&recipe.url, &path)?;
    remote::checkout(&path, &recipe.ref_)?;
  }

  let group = mold.open_group(group_name)?;
  clone(&group, recipe_name)
}

/// Find all dependencies for a given set of tasks
fn find_all_dependencies(
  mold: &Mold,
  targets: &TaskSet,
) -> Result<TaskSet, Error> {
  let mut new_targets = TaskSet::new();

  for target_name in targets {
    // insure we have it cloned already
    clone(mold, target_name)?;

    new_targets.extend(find_dependencies(mold, target_name)?);
    new_targets.insert(target_name.to_string());
  }

  Ok(new_targets)
}

/// Find all dependencies for a given task
fn find_dependencies(mold: &Mold, target: &str) -> Result<TaskSet, Error> {
  // check if this is a nested subrecipe that we'll have to recurse into
  if target.contains('/') {
    let splits: Vec<_> = target.splitn(2, '/').collect();
    let group_name = splits[0];
    let recipe_name = splits[1];

    let group = mold.open_group(group_name)?;
    let deps = find_dependencies(&group, recipe_name)?;
    let full_deps = find_all_dependencies(&group, &deps)?;
    return Ok(full_deps.iter().map(|x| format!("{}/{}", group_name, x)).collect());
  }

  // ...not a subrecipe
  let recipe = mold.find_recipe(target)?;
  let deps = recipe
    .dependencies()
    .iter()
    .map(|x| x.to_string())
    .collect();
  find_all_dependencies(mold, &deps)
}

/// Find a Task object for a given recipe name
fn find_task(
  mold: &Mold,
  target_name: &str,
  prev_env: &EnvMap,
) -> Result<Task, Error> {
  // check if we're executing a nested subrecipe that we'll have to recurse into
  if target_name.contains('/') {
    let splits: Vec<_> = target_name.splitn(2, '/').collect();
    let group_name = splits[0];
    let recipe_name = splits[1];
    let group = mold.open_group(group_name)?;

    // merge this moldfile's environment with its parent.
    // the parent has priority and overrides this moldfile because it's called recursively:
    //   $ mold foo/bar/baz
    // will call bar/baz with foo as the parent, which will call baz with bar as
    // the parent.  we want foo's moldfile to override bar's moldfile to override
    // baz's moldfile, because baz should be the least specialized.
    let mut env = group.data().environment.clone();
    env.extend(prev_env.into_iter().map(|(k, v)| (k.clone(), v.clone())));

    return find_task(&group, recipe_name, &env);
  }

  // ...not executing subrecipe, so look up the top-level recipe
  let recipe = mold.find_recipe(target_name)?;

  let task = match recipe {
    Recipe::Command(target) => Task::from_args(&target.command, Some(&prev_env)),
    Recipe::Script(target) => {
      // what the interpreter is for this recipe
      let type_ = mold.find_type(&target.type_)?;

      // find the script file to execute
      let script = match &target.script {
        Some(x) => {
          let mut path = mold.dir().clone();
          path.push(x);
          path
        }

        // we need to look it up based on our interpreter's known extensions
        None => type_.find(mold.dir(), &target_name)?,
      };

      type_.task(&script.to_str().unwrap(), prev_env)
    }
    Recipe::Group(_) => return Err(failure::err_msg("Can't execute a group")),
  };

  Ok(task)
}
