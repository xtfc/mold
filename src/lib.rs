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

// FIXME should Group / Script / Command have an optional "environment" override?
// FIXME should Group / Script / Command be able to document what environment vars they look at?

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
  /// A short description of the command
  #[serde(default)]
  pub help: String,

  /// A list of pre-execution dependencies
  #[serde(default)]
  pub deps: Vec<String>,

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

  /// A list of command arguments
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
  command: String,
  args: Vec<String>,
  env: Option<EnvMap>,
}

impl Mold {
  /// Open a moldfile and load it
  pub fn open(path: &Path) -> Result<Mold, Error> {
    let mut file = fs::File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data: Moldfile = toml::de::from_str(&contents)?;

    let mut dir = path.to_path_buf();
    dir.pop();
    dir.push(&data.recipe_dir);

    Ok(Mold {
      file: fs::canonicalize(path)?,
      dir: fs::canonicalize(dir)?,
      data: data,
    })
  }

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
  pub fn discover(name: &Path) -> Result<Mold, Error> {
    let path = Self::discover_file(name)?;
    Self::open(&path)
  }

  pub fn file(&self) -> &PathBuf {
    &self.file
  }

  pub fn dir(&self) -> &PathBuf {
    &self.dir
  }

  pub fn data(&self) -> &Moldfile {
    &self.data
  }

  pub fn env(&self) -> &EnvMap {
    &self.data.environment
  }


  /// Find a Recipe by name
  pub fn find_recipe(&self, target_name: &str) -> Result<&Recipe, Error> {
    self.data
      .recipes
      .get(target_name)
      .ok_or_else(|| failure::err_msg("couldn't locate target"))
  }

  /// Find a Type by name
  pub fn find_type(&self, type_name: &str) -> Result<&Type, Error> {
    self.data
      .types
      .get(type_name)
      .ok_or_else(|| failure::err_msg("couldn't locate type"))
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
    Self::discover(&self.dir.join(group_name).join(&target.file))
  }

  /// Print a description of all recipes in this moldfile
  pub fn help(&self) -> Result<(), Error> {
    // FIXME should this print things like dependencies?
    for (name, recipe) in &self.data.recipes {
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

  /// Print a dry run of the task and its environment
  pub fn dry(&self) {
    println!("{} {} {}", "$".green(), self.command, self.args.join(" "));
    if let Some(env) = &self.env {
      for (name, value) in env {
        println!(
          "  {} = \"{}\"",
          format!("${}", name).bright_cyan(),
          value
        );
      }
    }
  }

  /// Create a Task from a Vec of strings
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
  /// Create a Task ready to execute a script
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

  /// Attempt to discover an appropriate script in a recipe directory
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
  /// Return this recipe's dependencies
  pub fn dependencies(&self) -> Vec<String> {
    match self {
      Recipe::Script(s) => s.deps.clone(),
      Recipe::Command(c) => c.deps.clone(),
      _ => vec![],
    }
  }
}
