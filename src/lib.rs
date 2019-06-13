use colored::*;
use failure::Error;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::prelude::*;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::ffi::OsStr;

pub mod remote;

pub type RecipeMap = BTreeMap<String, Recipe>;
pub type TypeMap = BTreeMap<String, Type>;
pub type EnvMap = BTreeMap<String, String>;
pub type TaskSet = indexmap::IndexSet<String>;

#[derive(Debug)]
pub struct Mold {
  file: PathBuf,
  dir: PathBuf,
  data: Moldfile,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Moldfile {
  /// The directory that recipe scripts can be found in
  #[serde(default = "default_recipe_dir")]
  pub recipe_dir: String,

  /// A map of recipes
  #[serde(default)]
  pub recipes: RecipeMap,

  /// A map of interpreter types and characteristics
  #[serde(default)]
  pub types: TypeMap,

  /// A list of environment variables used to parametrize recipes
  #[serde(default)]
  pub environment: EnvMap,
}

fn default_recipe_dir() -> String {
  "./mold".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Recipe {
  Group(Group),
  Script(Script),
  Command(Command),
}

// FIXME Group / Script / Command should be able to document what environment vars they depend on

#[derive(Debug, Serialize, Deserialize)]
pub struct Group {
  /// A short description of the group's contents
  #[serde(default)]
  pub help: String,

  /// A list of environment variables that overrides the base environment
  #[serde(default)]
  pub environment: EnvMap,

  /// Git URL of a remote repo
  pub url: String,

  /// Git ref to keep up with
  #[serde(alias = "ref", default = "default_git_ref")]
  pub ref_: String,

  /// Moldfile to look at
  pub file: Option<String>,
}

fn default_git_ref() -> String {
  "master".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Script {
  /// A short description of the command
  #[serde(default)]
  pub help: String,

  /// A list of pre-execution dependencies
  #[serde(default)]
  pub deps: Vec<String>,

  /// A list of environment variables that overrides the base environment
  #[serde(default)]
  pub environment: EnvMap,

  /// Which interpreter should be used to execute this script
  #[serde(alias = "type")]
  pub type_: String,

  /// The script file name
  ///
  /// If left undefined, Mold will attempt to discover the recipe name by
  /// searching the recipe_dir for any files that start with the recipe name and
  /// have an appropriate extension for the specified interpreter type.
  pub script: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Command {
  /// A short description of the command
  #[serde(default)]
  pub help: String,

  /// A list of pre-execution dependencies
  #[serde(default)]
  pub deps: Vec<String>,

  /// A list of environment variables that overrides the base environment
  #[serde(default)]
  pub environment: EnvMap,

  /// A list of command arguments
  #[serde(default)]
  pub command: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Type {
  /// A list of arguments used as a shell command
  ///
  /// Any element "?" will be / replaced with the desired script when
  /// executing. eg:
  ///   ["python", "-m", "?"]
  /// will produce the shell command when .exec("foo") is called:
  ///   $ python -m foo
  pub command: Vec<String>,

  /// A list of extensions used to search for the script name
  ///
  /// These should omit the leading dot.
  #[serde(default)]
  pub extensions: Vec<String>,
}

#[derive(Debug)]
pub struct Task {
  args: Vec<String>,
  env: Option<EnvMap>,
}

impl Mold {
  /// Open a moldfile and load it
  pub fn open(path: &Path) -> Result<Mold, Error> {
    let mut file = fs::File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data: Moldfile = match path.extension().and_then(OsStr::to_str) {
      Some("yaml") => serde_yaml::from_str(&contents)?,
      _ => toml::de::from_str(&contents)?,
    };
    let dir = path.with_file_name(&data.recipe_dir);

    Ok(Mold {
      file: fs::canonicalize(path)?,
      dir: fs::canonicalize(dir)?,
      data,
    })
  }

  /// Try to locate a moldfile by walking up the directory tree
  fn discover_file(name: &Path) -> Result<PathBuf, Error> {
    if name.is_absolute() {
      if name.is_file() {
        return Ok(name.to_path_buf());
      } else {
        let name = format!("{}", name.display());
        return Err(failure::format_err!("File '{}' does not exist", name.red()));
      }
    }

    let mut path = std::env::current_dir()?;
    while !path.join(name).is_file() {
      path.pop();
      if path.parent().is_none() {
        break;
      }
    }

    path.push(name);

    if path.is_file() {
      Ok(path)
    } else {
      let name = format!("{}", name.display());
      Err(failure::format_err!("Unable to discover '{}'", name.red()))
    }
  }

  /// Try to locate a moldfile from a directory
  ///
  /// First checks for mold.toml, then moldfile, then Moldfile
  pub fn discover_dir(name: &Path) -> Result<Mold, Error> {
    let path = Self::discover_file(&name.join("mold.toml"))
      .or_else(|_| Self::discover_file(&name.join("moldfile")))
      .or_else(|_| Self::discover_file(&name.join("Moldfile")))
      .map_err(|_| {
        failure::format_err!(
          "Unable to discover '{}', '{}', or '{}'",
          "mold.toml".red(),
          "moldfile".red(),
          "Moldfile".red()
        )
      })?;
    Self::open(&path)
  }

  /// Try to locate a moldfile and load it
  pub fn discover(name: &Path) -> Result<Mold, Error> {
    let path = Self::discover_file(name)?;
    Self::open(&path)
  }

  pub fn env(&self) -> &EnvMap {
    &self.data.environment
  }

  /// Find a Recipe by name
  pub fn find_recipe(&self, target_name: &str) -> Result<&Recipe, Error> {
    self
      .data
      .recipes
      .get(target_name)
      .ok_or_else(|| failure::format_err!("Couldn't locate target '{}'", target_name.red()))
  }

  /// Find a Type by name
  pub fn find_type(&self, type_name: &str) -> Result<&Type, Error> {
    self
      .data
      .types
      .get(type_name)
      .ok_or_else(|| failure::format_err!("Couldn't locate type '{}'", type_name.red()))
  }

  /// Find a Recipe by name and attempt to unwrap it to a Group
  pub fn find_group(&self, group_name: &str) -> Result<&Group, Error> {
    // unwrap the group or quit
    match self.find_recipe(group_name)? {
      Recipe::Script(_) => Err(failure::err_msg("Requested recipe is a script")),
      Recipe::Command(_) => Err(failure::err_msg("Requested recipe is a command")),
      Recipe::Group(target) => Ok(target),
    }
  }

  pub fn open_group(&self, group_name: &str) -> Result<Mold, Error> {
    let target = self.find_group(group_name)?;
    match &target.file {
      Some(file) => Self::discover(&Path::new(file)),
      None => Self::discover_dir(&self.dir.join(group_name)),
    }
  }

  /// Recursively fetch/checkout for all groups that have already been cloned
  pub fn update_all(&self) -> Result<(), Error> {
    // find all groups that have already been cloned and update them.
    for (name, recipe) in &self.data.recipes {
      if let Recipe::Group(group) = recipe {
        let path = self.dir.join(name);

        // only update groups that have already been cloned
        if path.is_dir() {
          remote::checkout(&path, &group.ref_)?;

          // recursively update subgroups
          let group = self.open_group(name)?;
          group.update_all()?;
        }
      }
    }

    Ok(())
  }

  /// Clone all top-level targets
  pub fn clone_all(&self) -> Result<(), Error> {
    for (name, recipe) in &self.data.recipes {
      if let Recipe::Group(group) = recipe {
        let path = self.dir.join(name);
        if !path.is_dir() {
          remote::clone(&group.url, &path)?;
          remote::checkout(&path, &group.ref_)?;
        }
      }
    }

    Ok(())
  }

  /// Delete all cloned top-level targets
  pub fn clean_all(&self) -> Result<(), Error> {
    for (name, recipe) in &self.data.recipes {
      if let Recipe::Group(_) = recipe {
        let path = self.dir.join(name);
        if path.is_dir() {
          fs::remove_dir_all(&path)?;
          println!("{:>12} {}     ", "Deleted".red(), path.display());
        }
      }
    }

    Ok(())
  }

  /// Lazily clone groups for a given target
  pub fn clone(&self, target: &str) -> Result<(), Error> {
    // if this isn't a nested subrecipe, we don't need to worry about cloning anything
    if !target.contains('/') {
      return Ok(());
    }

    let splits: Vec<_> = target.splitn(2, '/').collect();
    let group_name = splits[0];
    let recipe_name = splits[1];

    let recipe = self.find_group(group_name)?;
    let path = self.dir.join(group_name);

    // if the directory doesn't exist, we need to clone it
    if !path.is_dir() {
      remote::clone(&recipe.url, &path)?;
      remote::checkout(&path, &recipe.ref_)?;
    }

    let group = self.open_group(group_name)?;
    group.clone(recipe_name)
  }

  /// Find all dependencies for a given set of tasks
  pub fn find_all_dependencies(&self, targets: &TaskSet) -> Result<TaskSet, Error> {
    let mut new_targets = TaskSet::new();

    for target_name in targets {
      // insure we have it cloned already
      self.clone(target_name)?;

      new_targets.extend(self.find_dependencies(target_name)?);
      new_targets.insert(target_name.to_string());
    }

    Ok(new_targets)
  }

  /// Find all dependencies for a given task
  fn find_dependencies(&self, target: &str) -> Result<TaskSet, Error> {
    // check if this is a nested subrecipe that we'll have to recurse into
    if target.contains('/') {
      let splits: Vec<_> = target.splitn(2, '/').collect();
      let group_name = splits[0];
      let recipe_name = splits[1];

      let group = self.open_group(group_name)?;
      let deps = group.find_dependencies(recipe_name)?;
      let full_deps = group.find_all_dependencies(&deps)?;
      return Ok(
        full_deps
          .iter()
          .map(|x| format!("{}/{}", group_name, x))
          .collect(),
      );
    }

    // ...not a subrecipe
    let recipe = self.find_recipe(target)?;
    let deps = recipe
      .deps()
      .iter()
      .map(std::string::ToString::to_string)
      .collect();
    self.find_all_dependencies(&deps)
  }

  /// Find a Task object for a given recipe name
  pub fn find_task(&self, target_name: &str, prev_env: &EnvMap) -> Result<Option<Task>, Error> {
    // check if we're executing a nested subrecipe that we'll have to recurse into
    if target_name.contains('/') {
      let splits: Vec<_> = target_name.splitn(2, '/').collect();
      let group_name = splits[0];
      let recipe_name = splits[1];
      let recipe = self.find_recipe(group_name)?;
      let group = self.open_group(group_name)?;

      // merge this moldfile's environment with its parent.
      // the parent has priority and overrides this moldfile because it's called recursively:
      //   $ mold foo/bar/baz
      // will call bar/baz with foo as the parent, which will call baz with bar as
      // the parent.  we want foo's moldfile to override bar's moldfile to override
      // baz's moldfile, because baz should be the least specialized.
      let mut env = group.env().clone();
      env.extend(prev_env.iter().map(|(k, v)| (k.clone(), v.clone())));

      let mut task = group.find_task(recipe_name, &env)?;
      if let Some(task) = &mut task {
        // not sure if this is the right ordering to update environments in, but
        // it's done here so that parent group's configuration can override one
        // of the subrecipes in the group
        if let Some(env) = &mut task.env {
          env.extend(recipe.env().iter().map(|(k, v)| (k.clone(), v.clone())));
        }
      }

      return Ok(task);
    }

    // ...not executing subrecipe, so look up the top-level recipe
    let recipe = self.find_recipe(target_name)?;

    // extend the environment with the recipe's environment settings
    let mut env = prev_env.clone();
    env.extend(recipe.env().iter().map(|(k, v)| (k.clone(), v.clone())));

    let task = match recipe {
      Recipe::Command(target) => Some(Task::from_args(&target.command, Some(&env))),
      Recipe::Script(target) => {
        // what the interpreter is for this recipe
        let type_ = self.find_type(&target.type_)?;

        // find the script file to execute
        let script = match &target.script {
          Some(x) => self.dir.join(x),

          // we need to look it up based on our interpreter's known extensions
          None => type_.find(&self.dir, &target_name)?,
        };

        Some(type_.task(&script.to_str().unwrap(), &env))
      }
      Recipe::Group(_) => {
        // this is kinda hacky, but... whatever. it should probably
        // somehow map into a Task instead, but this is good enough.
        let group_name = format!("{}/", target_name);
        self.clone(&group_name)?;
        let group = self.open_group(target_name)?;
        group.help_prefixed(&group_name)?;
        None
      }
    };

    Ok(task)
  }

  /// Print a description of all recipes in this moldfile
  pub fn help(&self) -> Result<(), Error> {
    self.help_prefixed("")
  }

  /// Print a description of all recipes in this moldfile
  pub fn help_prefixed(&self, prefix: &str) -> Result<(), Error> {
    for (name, recipe) in &self.data.recipes {
      let colored_name = match recipe {
        Recipe::Command(_) => name.yellow(),
        Recipe::Script(_) => name.cyan(),
        Recipe::Group(_) => name.magenta(),
      };

      // this is supposed to be 12 character padded, but after all the
      // formatting, we end up with a String instead of a
      // colored::ColoredString, so we can't get the padding correct.  but I'm
      // pretty sure that all the color formatting just adds 18 non-display
      // characters, so padding to 30 works out?
      let display_name: String = format!("{}{}", prefix.magenta(), colored_name);
      println!("{:>30} {}", display_name, recipe.help());

      // print dependencies
      let deps = recipe.deps();
      if !deps.is_empty() {
        println!(
          "             ⮡ {}",
          deps
            .iter()
            .map(|x| format!("{}{}", prefix, x))
            .collect::<Vec<_>>()
            .join(", ")
        );
      }

      if let Recipe::Group(_) = recipe {
        let path = self.dir.join(name);

        // only update groups that have already been cloned
        if path.is_dir() {
          let clear_name = match recipe {
            Recipe::Group(_) => format!("{}{}/", prefix, name),
            _ => format!("{}{}", prefix, name),
          };

          let group = self.open_group(name)?;
          group.help_prefixed(&clear_name)?;
        }
      }
    }

    Ok(())
  }
}

impl Task {
  /// Execute the task
  pub fn exec(&self) -> Result<(), Error> {
    if self.args.is_empty() {
      return Ok(());
    }

    let mut command = process::Command::new(&self.args[0]);
    command.args(&self.args[1..]);

    if let Some(env) = &self.env {
      command.envs(env);
    }

    let exit_status = command.spawn().and_then(|mut handle| handle.wait())?;

    if !exit_status.success() {
      return Err(failure::err_msg("recipe exited with non-zero code"));
    }

    Ok(())
  }

  /// Print the command to be executed
  pub fn print_cmd(&self) {
    if self.args.is_empty() {
      return;
    }

    println!("{} {}", "$".green(), self.args.join(" "));
  }

  /// Print the environment that will be used
  pub fn print_env(&self) {
    if self.args.is_empty() {
      return;
    }

    if let Some(env) = &self.env {
      for (name, value) in env {
        println!("  {} = \"{}\"", format!("${}", name).bright_cyan(), value);
      }
    }
  }

  /// Create a Task from a Vec of strings
  pub fn from_args(args: &[String], env: Option<&EnvMap>) -> Task {
    Task {
      args: args.to_owned(),
      env: env.map(std::clone::Clone::clone),
    }
  }
}

impl Type {
  /// Create a Task ready to execute a script
  pub fn task(&self, script: &str, env: &EnvMap) -> Task {
    let args: Vec<_> = self
      .command
      .iter()
      .map(|x| {
        if x == "?" {
          script.to_string()
        } else {
          x.to_string()
        }
      })
      .collect();

    Task {
      args,
      env: Some(env.clone()),
    }
  }

  /// Attempt to discover an appropriate script in a recipe directory
  pub fn find(&self, dir: &Path, name: &str) -> Result<PathBuf, Error> {
    // set up the pathbuf to look for dir/name
    let mut path = dir.join(name);

    // try all of our known extensions, early returning on the first match
    for ext in &self.extensions {
      path.set_extension(ext);
      if path.is_file() {
        return Ok(path);
      }
    }
    Err(failure::err_msg("Couldn't find a file"))
  }
}

impl Recipe {
  /// Return this recipe's dependencies
  pub fn deps(&self) -> Vec<String> {
    match self {
      Recipe::Script(s) => s.deps.clone(),
      Recipe::Command(c) => c.deps.clone(),
      _ => vec![],
    }
  }

  /// Return this recipe's help string
  pub fn help(&self) -> &str {
    match self {
      Recipe::Script(s) => &s.help,
      Recipe::Command(c) => &c.help,
      Recipe::Group(g) => &g.help,
    }
  }

  /// Return this recipe's environment
  pub fn env(&self) -> &EnvMap {
    match self {
      Recipe::Script(s) => &s.environment,
      Recipe::Command(c) => &c.environment,
      Recipe::Group(g) => &g.environment,
    }
  }
}
