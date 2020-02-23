pub mod lang;
pub mod remote;
pub mod util;

use colored::*;
use failure::Error;
use indexmap::indexmap;
use indexmap::IndexMap;
use indexmap::IndexSet;
use remote::Remote;
use semver::Version;
use semver::VersionReq;
use std::collections::BTreeMap;
use std::fs;
use std::io::prelude::*;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::string::ToString;

// maps sorted by insertion order
pub type IncludeVec = Vec<Include>;
pub type TargetSet = IndexSet<String>;
pub type EnvSet = IndexSet<String>;
pub type VarMap = IndexMap<String, String>; // TODO maybe down the line this should allow nulls to `unset` a variable
pub type SourceMap = IndexMap<String, PathBuf>;

// maps sorted alphabetically
pub type RecipeMap = BTreeMap<String, Recipe>;

pub const DEFAULT_FILES: &[&str] = &["moldfile", "Moldfile"];

pub struct Mold {
  /// A set of currently active environments
  pub envs: EnvSet,

  /// A map of recipes
  pub recipes: RecipeMap,

  /// A map of recipe sources
  pub sources: SourceMap,

  /// A map of environment variables
  pub vars: VarMap,

  /// List of Remotes that have been imported
  pub remotes: Vec<Remote>,

  /// Root of the origin moldfile
  pub root_dir: PathBuf,

  /// Path to cloned repos and generated scripts
  pub mold_dir: PathBuf,
}

pub struct Include {
  /// Remote to include
  pub remote: Remote,

  /// Prefix to prepend
  pub prefix: String,
}

// FIXME script
#[derive(Clone)]
pub struct Recipe {
  /// A short description of the recipe
  pub help: Option<String>,

  /// Working directory relative to $MOLD_ROOT
  pub dir: Option<String>,

  /// The command to execute
  pub commands: Vec<String>,

  /// A list of environment variables
  pub vars: VarMap,

  /// A list of prerequisite recipes
  pub requires: TargetSet,
}

pub struct Moldfile {
  pub version: String,
  pub includes: IncludeVec,
  pub recipes: RecipeMap,
  pub vars: VarMap,
}

// Moldfiles
impl Mold {
  pub fn init(path: &Path, envs: Vec<String>) -> Result<Mold, Error> {
    let root_dir = path.parent().unwrap_or(&Path::new("/")).to_path_buf();
    let mold_dir = root_dir.join(".mold");

    if !mold_dir.is_dir() {
      fs::create_dir(&mold_dir)?;
    }

    let vars = indexmap! {
      "MOLD_ROOT".into() => root_dir.to_string_lossy().into(),
      "MOLD_DIR".into() => mold_dir.to_string_lossy().into(),
    };

    let envs = envs.into_iter().collect();

    let mut mold = Mold {
      root_dir: fs::canonicalize(root_dir)?,
      mold_dir: fs::canonicalize(mold_dir)?,
      recipes: RecipeMap::new(),
      sources: SourceMap::new(),
      remotes: vec![],
      envs,
      vars,
    };

    mold.open(path, "")?;

    Ok(mold)
  }

  /// Delete all cloned top-level targets
  pub fn clean_all(path: &Path) -> Result<(), Error> {
    let root_dir = path.parent().unwrap_or(&Path::new("/")).to_path_buf();
    let mold_dir = root_dir.join(".mold");

    if mold_dir.is_dir() {
      fs::remove_dir_all(&mold_dir)?;
      println!("{:>12} {}", "Deleted".red(), mold_dir.display());
    } else {
      println!("{:>12}", "Clean!".green());
    }

    Ok(())
  }

  /// Given a path, open and parse the file
  fn open(&mut self, path: &Path, prefix: &str) -> Result<(), Error> {
    let mut file = fs::File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data = self::lang::compile(&contents, &self.envs)?;
    let root_dir = path.parent().unwrap_or(&Path::new("/")).to_path_buf();

    // check version requirements
    let self_version = Version::parse(clap::crate_version!())?;
    let target_version = VersionReq::parse(&data.version)?;
    if !target_version.matches(&self_version) {
      return Err(failure::format_err!(
        "Incompatible versions: file {} requires version {}, but current version is {}",
        path.to_str().unwrap().blue(),
        target_version.to_string().green(),
        self_version.to_string().red()
      ));
    }

    for (name, recipe) in data.recipes {
      let new_key = format!("{}{}", prefix, name);

      // clone this recipe and prefix all of its dependencies
      let mut new_recipe = recipe.clone();
      new_recipe.requires = new_recipe
        .requires
        .iter()
        .map(|x| format!("{}{}", prefix, x))
        .collect();

      self.recipes.entry(new_key.clone()).or_insert(new_recipe);

      // keep track of where this recipe came from so it can use things from
      // its repo
      self.sources.entry(new_key).or_insert(root_dir.clone());
    }

    for include in data.includes {
      if !include.remote.exists(&self.mold_dir) {
        include.remote.pull(&self.mold_dir)?;
        include.remote.checkout(&self.mold_dir)?;
      }

      let path = include.remote.path(&self.mold_dir);
      self.remotes.push(include.remote.clone());
      let filepath = Self::discover(&path, include.remote.file)?;
      self.open(&filepath, &include.prefix)?;
    }

    self.vars.extend(data.vars);

    Ok(())
  }

  /// Try to find a file by walking up the tree
  ///
  /// Absolute paths will either be located or fail instantly. Relative paths
  /// will walk the entire file tree up to root, looking for a file with the
  /// given name.
  fn discover_file(name: &Path) -> Result<PathBuf, Error> {
    // if it's an absolute path, we don't need to walk up the tree.
    if name.is_absolute() {
      if name.is_file() {
        return Ok(name.to_path_buf());
      } else if name.exists() {
        let name = format!("{}", name.display());
        return Err(failure::format_err!(
          "'{}' exists, but is not a file",
          name.red()
        ));
      } else {
        let name = format!("{}", name.display());
        return Err(failure::format_err!("File '{}' does not exist", name.red()));
      }
    }

    // walk up the tree until we find the file or hit the root
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
      Err(failure::format_err!("Unable to locate a '{}'", name.red()))
    }
  }

  /// Search a directory for default moldfiles
  ///
  /// Iterates through all values found in `DEFAULT_FILES`, joining them to the
  /// provided `name` argument
  fn discover_dir(name: &Path) -> Result<PathBuf, Error> {
    let path = DEFAULT_FILES
      .iter()
      .find_map(|file| Self::discover_file(&name.join(file)).ok())
      .ok_or_else(|| {
        failure::format_err!(
          "Cannot locate moldfile, tried the following:\n{}",
          DEFAULT_FILES.join(" ").red()
        )
      })?;
    Ok(path)
  }

  /// Try to locate a file or a directory, opening it if found
  pub fn discover(dir: &Path, file: Option<PathBuf>) -> Result<PathBuf, Error> {
    // I think this should take Option<&Path> but I couldn't figure out how to
    // please the compiler when I have an existing Option<PathBuf>, so...  I'm
    // just using .clone() on it.
    match file {
      Some(file) => Self::discover_file(&dir.join(file)),
      None => Self::discover_dir(dir),
    }
  }

  /// Find a recipe in the top level map
  fn recipe(&self, name: &str) -> Result<&Recipe, Error> {
    self
      .recipes
      .get(name)
      .ok_or_else(|| failure::format_err!("Couldn't locate recipe '{}'", name.red()))
  }

  pub fn execute(&self, name: &str) -> Result<(), Error> {
    let recipe = self.recipe(name)?;

    let mut vars = self.vars.clone();
    vars.extend(recipe.vars.clone());

    // insert var for where this recipe's mold file lives
    if let Some(source) = self.sources.get(name) {
      vars.insert("MOLD_SOURCE".into(), source.to_string_lossy().into());
    } else {
      return Err(failure::format_err!(
        "Couldn't locate source for recipe '{}'",
        name.red()
      ));
    }

    for command_str in &recipe.commands {
      let args = self.build_args(command_str, &vars)?;

      if args.is_empty() {
        continue;
      }

      let mut command = process::Command::new(&args[0]);
      command.args(&args[1..]);
      command.envs(&vars);

      // FIXME this should be relative to root, no?
      if let Some(dir) = &recipe.dir {
        command.current_dir(self.root_dir.join(dir));
      }

      println!(
        "{} {} {} {}",
        "mold".white(),
        name.cyan(),
        "$".green(),
        shell_words::join(&args),
      );

      let exit_status = command.spawn().and_then(|mut handle| handle.wait())?;
      if !exit_status.success() {
        return Err(failure::err_msg("Recipe returned non-zero exit status"));
      }
    }

    Ok(())
  }

  /// Perform variable expansion and return a list of arguments to pass to Command
  fn build_args(&self, command: &str, vars: &VarMap) -> Result<Vec<String>, Error> {
    let expanded = shellexpand::env_with_context_no_errors(&command, |name| {
      vars
        .get(name)
        .map(std::string::ToString::to_string)
        .or_else(|| std::env::var(name).ok())
        .or_else(|| Some("".into()))
    });
    Ok(shell_words::split(&expanded)?)
  }

  pub fn find_all_dependencies(&self, targets: &TargetSet) -> Result<TargetSet, Error> {
    let mut new_targets = TargetSet::new();

    // FIXME this might not break on weird infinite cycles
    // ...but since those shouldn't happen in sanely written moldfiles...
    for target_name in targets {
      new_targets.extend(self.find_dependencies(target_name)?);
      new_targets.insert(target_name.clone());
    }

    Ok(new_targets)
  }

  fn find_dependencies(&self, target_name: &str) -> Result<TargetSet, Error> {
    let recipe = self.recipe(target_name)?;
    let deps = recipe.requires.iter().map(ToString::to_string).collect();
    self.find_all_dependencies(&deps)
  }

  /// Update (ie: fetch + force checkout) all remotes
  pub fn update_all(&self) -> Result<(), Error> {
    for remote in &self.remotes {
      let path = remote.path(&self.mold_dir);
      if path.is_dir() {
        remote.checkout(&self.mold_dir)?;
      }
    }

    Ok(())
  }
}

/*
// Recipes
impl Mold {
  fn script_name(&self, recipe: &Recipe) -> Result<Option<PathBuf>, Error> {
    if let Some(script) = &recipe.script {
      let file = self.mold_dir.join(util::hash_string(&script));
      fs::write(&file, &script)?;
      Ok(Some(file))
    } else {
      Ok(None)
    }
  }
}
*/

// Remotes

/*
// Help
impl Mold {
  /// Print a description of all recipes in this moldfile
  pub fn help(&self) -> Result<(), Error> {
    for (name, recipe) in &self.data.recipes {
      println!("{:>12} {}", name.cyan(), recipe.help());

      // print dependencies
      let deps = recipe.deps();
      if !deps.is_empty() {
        println!("             ⮡ {}", deps.join(" ").cyan());
      }
    }

    Ok(())
  }

  /// Print an explanation of global settings for this Moldfile
  pub fn explain_self(&self) -> Result<(), Error> {
    println!("{:12} {}", "environments:".white(), self.envs.join(" "));

    if !self.data.environments.is_empty() {
      println!("{:12}", "conditionals:".white());

      let active = active_envs(&self.data.environments, &self.envs);

      for (cond, map) in &self.data.environments {
        let cond_disp = if active.contains(cond) {
          cond.green()
        } else {
          cond.blue()
        };

        println!("  {}:", cond_disp);
        for (key, val) in map {
          println!("    {:16} = {}", format!("${}", key).bright_cyan(), val);
        }
      }
    }

    let vars = self.env_vars();

    if !vars.is_empty() {
      println!("{:12}", "variables:".white());

      for (key, val) in &vars {
        println!("  {:16} = {}", format!("${}", key).bright_cyan(), val);
      }
    }

    println!();

    Ok(())
  }

  /// Print an explanation of what a recipe does
  pub fn explain(&self, target_name: &str) -> Result<(), Error> {
    let recipe = self.find_recipe(target_name)?;

    println!("{:12}", target_name.cyan());
    if !recipe.help().is_empty() {
      println!("{:12} {}", "help:".white(), recipe.help());
    }

    if !recipe.deps().is_empty() {
      println!(
        "{:12} {}",
        "depends on:".white(),
        recipe.deps().join(" ").cyan()
      );
    }

    if let Some(dir) = recipe.work_dir() {
      println!(
        "{:12} {}",
        "working dir:".white(),
        dir.display().to_string().cyan()
      );
    }

    println!("{:12} {}", "command:".white(), recipe.shell(&self.envs)?);

    let task = self.build_task(target_name)?;

    println!("{:12}", "variables:".white());
    for (name, desc) in &task.vars {
      println!(
        "  {}{} {}",
        format!("${}", name).bright_cyan(),
        ":".white(),
        desc
      );
    }

    println!(
      "{:12} {} {}",
      "executes:".white(),
      "$".green(),
      task.args.join(" ")
    );

    // display contents of script file
    if let Some(script) = self.script_name(recipe)? {
      util::cat(script)?;
    }

    println!();

    Ok(())
  }
}
*/
