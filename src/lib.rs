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

// FIXME working dir
// FIXME script
// FIXME dependencies
pub struct Recipe {
  /// A short description of the module's contents
  pub help: Option<String>,

  /// The command to execute
  pub commands: Vec<String>,

  /// A list of environment variables
  pub vars: VarMap,
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
      "MOLD_ROOT".to_string() => root_dir.to_string_lossy().to_string(),
      "MOLD_DIR".to_string() => mold_dir.to_string_lossy().to_string(),
    };

    let envs = envs.into_iter().collect();

    let mut mold = Mold {
      root_dir: fs::canonicalize(root_dir)?,
      mold_dir: fs::canonicalize(mold_dir)?,
      recipes: RecipeMap::new(),
      sources: SourceMap::new(),
      envs,
      vars,
    };

    mold.open(path, "")?;

    Ok(mold)
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
      self.sources.insert(new_key.clone(), root_dir.clone());
      self.recipes.entry(new_key).or_insert(recipe);
    }

    for include in data.includes {
      if !include.remote.exists(&self.mold_dir) {
        include.remote.clone(&self.mold_dir)?;
        include.remote.checkout(&self.mold_dir)?;
      }

      let path = include.remote.path(&self.mold_dir);
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

  /// Delete all cloned top-level targets
  pub fn clean_all(&self) -> Result<(), Error> {
    // no point in checking if it exists, because Mold::open creates it
    fs::remove_dir_all(&self.mold_dir)?;
    println!("{:>12} {}", "Deleted".red(), self.mold_dir.display());
    Ok(())
  }

  /// Find a recipe in the top level map
  fn find_recipe(&self, target_name: &str) -> Result<&Recipe, Error> {
    self
      .recipes
      .get(target_name)
      .ok_or_else(|| failure::format_err!("Couldn't locate target '{}'", target_name.red()))
  }

  pub fn execute(&self, target_name: &str) -> Result<(), Error> {
    let recipe = self.find_recipe(target_name)?;

    let mut vars = self.vars.clone();
    vars.extend(recipe.vars.clone());

    for command_str in &recipe.commands {
      let args = self.build_args(command_str, &vars)?;

      if args.is_empty() {
        continue;
      }

      let mut command = process::Command::new(&args[0]);
      command.args(&args[1..]);
      command.envs(&vars);

      /*
      // FIXME this should be relative to root, no?
      if let Some(dir) = &recipe.work_dir {
        command.current_dir(dir);
      }
      */

      println!(
        "{} {} {} {}",
        "mold".white(),
        target_name.cyan(),
        "$".green(),
        shell_words::join(&args),
      );

      let exit_status = command.spawn().and_then(|mut handle| handle.wait())?;
      if !exit_status.success() {
        return Err(failure::err_msg("recipe returned non-zero exit status"));
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
}

/*
// Recipes
impl Mold {
  pub fn find_all_dependencies(&self, targets: &TargetSet) -> Result<TargetSet, Error> {
    let mut new_targets = TargetSet::new();

    for target_name in targets {
      new_targets.extend(self.find_dependencies(target_name)?);
      new_targets.insert(target_name.clone());
    }

    Ok(new_targets)
  }

  fn find_dependencies(&self, target_name: &str) -> Result<TargetSet, Error> {
    let recipe = self.find_recipe(target_name)?;
    let deps = recipe.deps().iter().map(ToString::to_string).collect();
    self.find_all_dependencies(&deps)
  }

  /// Return a list of arguments to pass to Command
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

/*
// Remotes
impl Mold {
  /// Update a single remote
  ///
  /// * find the expected path
  /// * make sure it exists (ie, is cloned) and hasn't been visited
  /// * track it as visited
  /// * fetch / checkout
  /// * recurse into it
  fn update_remote(&self, remote: &Remote, updated: &mut HashSet<PathBuf>) -> Result<(), Error> {
    let path = self.mold_dir.join(remote.folder_name());
    if path.is_dir() && !updated.contains(&path) {
      updated.insert(path.clone());
      remote::checkout(&path, &remote.ref_)?;
      self.open_remote(remote)?.update_all_track(updated)?;
    }

    Ok(())
  }

  /// Recursively fetch/checkout for all modules that have already been cloned
  pub fn update_all(&self) -> Result<(), Error> {
    self.update_all_track(&mut HashSet::new())
  }

  /// Recursively fetch/checkout for all modules that have already been cloned,
  /// but with extra checks to avoid infinite recursion cycles
  fn update_all_track(&self, updated: &mut HashSet<PathBuf>) -> Result<(), Error> {
    // `updated` contains all of the directories that have been, well, updated.
    // it *needs* to be passed to recursive calls.

    // find all Includes that have already been cloned and update them
    for include in &self.data.includes {
      self.update_remote(&include.remote, updated)?;
    }

    Ok(())
  }
}
*/

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

/*
impl Moldfile {
  /// Merges any recipes from `other` that aren't in `self`
  pub fn merge(&mut self, other: Mold, prefix: &str) {
    for (recipe_name, recipe) in other.data.recipes {
      let mut new_recipe = recipe.clone();
      new_recipe.deps = new_recipe
        .deps
        .iter()
        .map(|x| format!("{}{}", prefix, x))
        .collect();

      self
        .recipes
        .entry(format!("{}{}", prefix, recipe_name))
        .or_insert(new_recipe);
    }
  }
}
*/
