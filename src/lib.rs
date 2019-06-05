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
  /// Git URL of a remote repo
  pub url: String,

  /// Git ref to keep up with
  #[serde(alias = "ref", default = "default_git_ref")]
  pub ref_: String,

  /// Moldfile to look at
  #[serde(default = "default_moldfile")]
  pub file: String,

  /// A short description of the group's contents
  #[serde(default)]
  pub help: String,
}

fn default_git_ref() -> String {
  "master".to_string()
}

fn default_moldfile() -> String {
  "moldfile".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Script {
  /// Which interpreter should be used to execute this script.
  #[serde(alias = "type")]
  pub type_: String,

  /// A short description of the command.
  #[serde(default)]
  pub help: String,

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

  /// Try to locate a moldfile and load it
  pub fn discover(name: &Path) -> Result<Moldfile, Error> {
    let path = Moldfile::discover_file(name)?;
    let mut file = fs::File::open(&path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let data: Moldfile = toml::de::from_str(&contents)?;
    Ok(data)
  }

  /// Return the directory that contains the mold scripts
  pub fn mold_dir(&self, root: &Path) -> Result<PathBuf, Error> {
    let mut path = root.to_path_buf();
    path.pop();
    path.push(&self.recipe_dir);
    Ok(fs::canonicalize(path)?)
  }
}

impl Type {
  /// Execute a file using self.command
  pub fn exec(&self, cmd: &str, env: &EnvMap) -> Result<(), Error> {
    let args: Vec<_> = self
      .command
      .iter()
      .map(|x| if x == "?" { cmd } else { x })
      .collect();

    exec(args, env)?;

    Ok(())
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

/// Execute an external command
pub fn exec(cmd: Vec<&str>, env: &EnvMap) -> Result<(), Error> {
  let mut args = cmd.clone();
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
