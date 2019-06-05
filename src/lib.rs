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

pub mod remote;

pub type RecipeMap = BTreeMap<String, Recipe>;
pub type TypeMap = BTreeMap<String, Type>;
pub type EnvMap = BTreeMap<String, String>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Moldfile {
  /// The directory that recipe scripts can be found in
  #[serde(default = "default_recipe_dir")]
  pub recipe_dir: String,

  /// A map of recipes.
  #[serde(default)]
  pub recipes: RecipeMap,

  /// A map of interpreter types and characteristics.
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Group {
  /// A short description of the group's contents
  #[serde(default)]
  pub help: String,

  /// Git URL of a remote repo
  pub url: String,

  /// Git ref to keep up with
  #[serde(alias = "ref", default = "default_git_ref")]
  pub ref_: String,

  /// Moldfile to look at
  #[serde(default = "default_moldfile")]
  pub file: String,
}

fn default_git_ref() -> String {
  "master".to_string()
}

fn default_moldfile() -> String {
  "moldfile".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Script {
  /// A short description of the command.
  #[serde(default)]
  pub help: String,

  /// A list of pre-execution dependencies
  #[serde(default)]
  pub deps: Vec<String>,

  /// Which interpreter should be used to execute this script.
  #[serde(alias = "type")]
  pub type_: String,

  /// The script file name.
  ///
  /// If left undefined, Mold will attempt to discover the recipe name by
  /// searching the recipe_dir for any files that start with the recipe name and
  /// have an appropriate extension for the specified interpreter type.
  pub script: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Command {
  /// A short description of the command.
  #[serde(default)]
  pub help: String,

  /// A list of pre-execution dependencies
  #[serde(default)]
  pub deps: Vec<String>,

  /// A list of command arguments
  pub command: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Type {
  /// A list of arguments used as a shell command.
  ///
  /// Any element "?" will be / replaced with the desired script when
  /// executing. eg:
  ///   ["python", "-m", "?"]
  /// will produce the shell command when .exec("foo") is called:
  ///   $ python -m foo
  pub command: Vec<String>,

  /// A list of extensions used to search for the script name.
  ///
  /// These should omit the leading dot.
  #[serde(default)]
  pub extensions: Vec<String>,
}

#[derive(Debug)]
pub struct Task {
  command: String,
  args: Vec<String>,
  env: Option<EnvMap>,
}

impl Moldfile {
  /// Try to locate a moldfile by walking up the directory tree
  fn discover_file(name: &Path) -> Result<PathBuf, Error> {
    let mut path = std::env::current_dir()?;
    while !path.join(name).is_file() {
      path.pop();
    }

    path.push(name);

    if path.is_file() {
      Ok(path)
    } else {
      Err(failure::err_msg("Unable to discover a moldfile"))
    }
  }

  /// Try to open a moldfile and load it
  pub fn open(path: &Path) -> Result<Moldfile, Error> {
    let mut file = fs::File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let data: Moldfile = toml::de::from_str(&contents)?;
    Ok(data)
  }

  /// Try to locate a moldfile and load it
  pub fn discover(name: &Path) -> Result<Moldfile, Error> {
    let path = Moldfile::discover_file(name)?;
    Moldfile::open(&path)
  }

  /// Return the directory that contains the mold scripts
  pub fn mold_dir(&self, root: &Path) -> Result<PathBuf, Error> {
    let mut path = root.to_path_buf();
    path.pop();
    path.push(&self.recipe_dir);
    Ok(fs::canonicalize(path)?)
  }

  pub fn find_recipe(&self, target_name: &str) -> Result<&Recipe, Error> {
    self
      .recipes
      .get(target_name)
      .ok_or_else(|| failure::err_msg("couldn't locate target"))
  }

  pub fn find_type(&self, type_name: &str) -> Result<&Type, Error> {
    self
      .types
      .get(type_name)
      .ok_or_else(|| failure::err_msg("couldn't locate type"))
  }

  pub fn find_group(&self, root: &Path, group_name: &str) -> Result<&Group, Error> {
    // unwrap the group or quit
    match self.find_recipe(group_name)? {
      Recipe::Script(_) => Err(failure::err_msg("Can't find moldfile for a script")),
      Recipe::Command(_) => Err(failure::err_msg("Can't find moldfile for a command")),
      Recipe::Group(target) => Ok(target),
    }
  }

  pub fn find_group_file(&self, root: &Path, group_name: &str) -> Result<PathBuf, Error> {
    let target = self.find_group(root, group_name)?;
    Moldfile::discover_file(&self.mold_dir(root)?.join(group_name).join(&target.file))
  }

  /// Print a description of all recipes in this moldfile
  pub fn help(&self) -> Result<(), Error> {
    for (name, recipe) in &self.recipes {
      let (name, help) = match recipe {
        Recipe::Command(c) => (name.yellow(), &c.help),
        Recipe::Script(s) => (name.cyan(), &s.help),
        Recipe::Group(g) => (format!("{}/", name).magenta(), &g.help),
      };
      println!("{:>12} {}", name, help);
    }

    Ok(())
  }
}

impl Task {
  /// Execute the task
  pub fn exec(&self) -> Result<(), Error> {
    let mut command = process::Command::new(&self.command);
    command.args(&self.args[..]);

    if let Some(env) = &self.env {
      command.envs(env);
    }

    let exit_status = command.spawn().and_then(|mut handle| handle.wait())?;

    if !exit_status.success() {
      return Err(failure::err_msg("recipe exited with non-zero code"));
    }

    Ok(())
  }

  pub fn from_args(args: &Vec<String>, env: Option<&EnvMap>) -> Task {
    let mut args = args.clone();
    // FIXME panics if args is empty
    let cmd = args.remove(0);
    Task {
      command: cmd,
      args: args,
      env: env.map(|x| x.clone()),
    }
  }
}

impl Type {
  /// Execute a file using self.command
  pub fn exec(&self, script: &str, env: &EnvMap) -> Result<(), Error> {
    let args: Vec<_> = self
      .command
      .iter()
      .map(|x| if x == "?" { script } else { x })
      .collect();

    exec(args, env)?;

    Ok(())
  }

  pub fn task(&self, script: &str, env: &EnvMap) -> Task {
    let mut args: Vec<_> = self
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

    // FIXME panics if args is empty
    let cmd = args.remove(0);

    Task {
      command: cmd,
      args: args,
      env: Some(env.clone()),
    }
  }

  /// Attempt to discover an appropriate script in a recipe directory.
  pub fn find(&self, dir: &Path, name: &str) -> Result<PathBuf, Error> {
    // set up the pathbuf to look for dir/name
    let mut pb = dir.to_path_buf();
    pb.push(name);

    // try all of our known extensions, early returning on the first match
    for ext in &self.extensions {
      pb.set_extension(ext);
      if pb.is_file() {
        return Ok(pb);
      }
    }
    Err(failure::err_msg("Couldn't find a file"))
  }
}

impl Recipe {
  pub fn dependencies(&self) -> Vec<String> {
    match self {
      Recipe::Script(s) => s.deps.clone(),
      Recipe::Command(c) => c.deps.clone(),
      _ => vec![],
    }
  }
}

/// Execute an external command
pub fn exec(cmd: Vec<&str>, env: &EnvMap) -> Result<(), Error> {
  let mut args = cmd.clone();
  // FIXME panics if args is empty
  let command = args.remove(0);

  let exit_status = process::Command::new(&command)
    .args(&args[..])
    .envs(env)
    .spawn()
    .and_then(|mut handle| handle.wait())?;

  if !exit_status.success() {
    return Err(failure::err_msg("recipe exited with non-zero code"));
  }

  Ok(())
}
