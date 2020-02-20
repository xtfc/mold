pub mod file;
pub mod remote;
pub mod serde;
pub mod util;

use colored::*;
use failure::Error;
use file::Command;
use file::Moldfile;
use file::Recipe;
use file::Remote;
use file::TargetSet;
use file::VarMap;
use file::DEFAULT_FILES;
use indexmap::IndexMap;
use semver::Version;
use semver::VersionReq;
use std::collections::HashSet;
use std::fs;
use std::io::prelude::*;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::str::FromStr;
use std::string::ToString;

/// Generate a list of all active environments
///
/// Environment map keys are parsed as test expressions and evaluated against
/// the list of environments. Environments that evaluate to true are added to
/// the returned list; environments that evaluate to false are ignored.
fn active_envs(env_map: &file::EnvMap, envs: &[String]) -> Vec<String> {
  let mut result = vec![];
  for (test, _) in env_map {
    match serde::compile_expr(&test) {
      Ok(ex) => {
        if ex.apply(&envs) {
          result.push(test.clone());
        }
      }
      // FIXME this error handling should probably be better
      Err(err) => println!("{}: '{}': {}", "Warning".bright_red(), test, err),
    }
  }
  result
}

#[derive(Debug)]
pub struct Mold {
  /// path to the moldfile
  file: PathBuf,

  /// (derived) root directory that the file sits in
  root_dir: PathBuf,

  /// (derived) path to the cloned repos and generated scripts
  mold_dir: PathBuf,

  /// which environments to use in the environment
  envs: Vec<String>,

  /// the parsed moldfile data
  data: file::Moldfile,
}

#[derive(Debug)]
pub struct Task {
  args: Vec<String>,
  vars: VarMap,
  work_dir: Option<PathBuf>,
}

// Moldfiles
impl Mold {
  /// Given a path, open and parse the file
  pub fn open(path: &Path) -> Result<Mold, Error> {
    let mut file = fs::File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data = self::serde::from_str(&contents)?;

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

    let root_dir = path.parent().unwrap_or(&Path::new("/")).to_path_buf();
    let mold_dir = root_dir.join(".mold");

    if !mold_dir.is_dir() {
      fs::create_dir(&mold_dir)?;
    }

    Ok(Mold {
      file: fs::canonicalize(path)?,
      root_dir: fs::canonicalize(root_dir)?,
      mold_dir: fs::canonicalize(mold_dir)?,
      envs: vec![],
      data,
    })
  }

  /// Try to find a file by walking up the tree
  ///
  /// Absolute paths will either be located or fail instantly. Relative paths
  /// will walk the entire file tree up to root, looking for a file with the
  /// given name.
  fn find_file(name: &Path) -> Result<PathBuf, Error> {
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

  /// Search a directory for default moldfiles, opening it if found
  ///
  /// Iterates through all values found in `DEFAULT_FILES`, joining them to the
  /// provided `name` argument
  fn discover_dir(name: &Path) -> Result<Mold, Error> {
    let path = DEFAULT_FILES
      .iter()
      .find_map(|file| Self::find_file(&name.join(file)).ok())
      .ok_or_else(|| {
        failure::format_err!(
          "Cannot locate moldfile, tried the following:\n{}",
          DEFAULT_FILES.join(" ").red()
        )
      })?;
    Self::open(&path)
  }

  /// Try to locate a moldfile by name, opening it if found
  fn discover_file(name: &Path) -> Result<Mold, Error> {
    let path = Self::find_file(name)?;
    Self::open(&path)
  }

  /// Try to locate a file or a directory, opening it if found
  pub fn discover(dir: &Path, file: Option<PathBuf>) -> Result<Mold, Error> {
    // I think this should take Option<&Path> but I couldn't figure out how to
    // please the compiler when I have an existing Option<PathBuf>, so...  I'm
    // just using .clone() on it.
    match file {
      Some(file) => Self::discover_file(&dir.join(file)),
      None => Self::discover_dir(dir),
    }
  }
}

// Environments
impl Mold {
  /// Return this moldfile's variables with activated environments
  ///
  /// This also inserts a few special mold variables
  fn env_vars(&self) -> VarMap {
    let active = active_envs(&self.data.environments, &self.envs);

    let mut vars = self.data.variables.clone();
    for env_name in active {
      if let Some(env) = self.data.environments.get(&env_name) {
        vars.extend(env.iter().map(|(k, v)| (k.clone(), v.clone())));
      }
    }

    vars
  }

  pub fn set_envs(&mut self, env: Option<String>) {
    self.envs = match env {
      Some(envs) => envs.split(',').map(|x| x.into()).collect(),
      None => vec![],
    };
  }

  pub fn add_envs(&mut self, envs: Vec<String>) {
    self.envs.extend(envs);
  }

  pub fn add_env(&mut self, env: &str) {
    self.envs.push(env.into());
  }
}

// Recipes
impl Mold {
  /// Find a recipe in the top level map
  fn find_recipe(&self, target_name: &str) -> Result<&Recipe, Error> {
    self
      .data
      .recipes
      .get(target_name)
      .ok_or_else(|| failure::format_err!("Couldn't locate target '{}'", target_name.red()))
  }

  fn open_remote(&self, target: &Remote) -> Result<Mold, Error> {
    let path = self.mold_dir.join(&target.folder_name());
    let mut mold = Self::discover(&path, target.file.clone())?.adopt(self);
    mold.process_includes()?;
    Ok(mold)
  }

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

  pub fn build_task(&self, target_name: &str) -> Result<Task, Error> {
    let recipe = self.find_recipe(target_name)?;
    let vars = self.build_vars(recipe)?;
    let args = self.build_args(recipe, &vars)?;
    Ok(Task {
      work_dir: recipe.work_dir.clone(),
      vars,
      args,
    })
  }

  /// Perform variable expansion and return a list of arguments to pass to Command
  fn build_args(&self, recipe: &Recipe, vars: &VarMap) -> Result<Vec<String>, Error> {
    let command = recipe.shell(&self.envs)?;
    let expanded = shellexpand::env_with_context_no_errors(&command, |name| {
      vars
        .get(name)
        .map(std::string::ToString::to_string)
        .or_else(|| std::env::var(name).ok())
        .or_else(|| Some("".into()))
    });
    Ok(shell_words::split(&expanded)?)
  }

  /// Return a list of arguments to pass to Command
  fn build_vars(&self, recipe: &Recipe) -> Result<VarMap, Error> {
    let mut vars = self.env_vars();
    vars.extend(
      self
        .mold_vars(recipe)?
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string())),
    );
    Ok(vars)
  }

  /// Return a list of arguments to pass to Command
  fn mold_vars(&self, recipe: &Recipe) -> Result<VarMap, Error> {
    let mut vars = IndexMap::new();

    vars.insert("MOLD_ROOT", self.root_dir.to_string_lossy());
    vars.insert("MOLD_FILE", self.file.to_string_lossy());
    vars.insert("MOLD_DIR", self.mold_dir.to_string_lossy());

    if let Some(script) = self.script_name(recipe)? {
      // what the fuck is going on here?
      // PathBuf -> String is such a nightmare.
      // it seems like the .to_string().into() is needed to satisfy borrowck.
      vars.insert("MOLD_SCRIPT", script.to_string_lossy().to_string().into());
    }

    let ret: VarMap = vars
      .iter()
      .map(|(k, v)| ((*k).to_string(), v.to_string()))
      .collect();
    Ok(ret)
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

// Remotes
impl Mold {
  /// Clone a single remote reference and then recursively clone subremotes
  fn clone(
    &self,
    folder_name: &str,
    url: &str,
    ref_: &str,
    file: Option<PathBuf>,
  ) -> Result<(), Error> {
    let path = self.mold_dir.join(folder_name);
    if !path.is_dir() {
      remote::clone(&format!("https://{}", url), &path).or_else(|_| remote::clone(url, &path))?;
      remote::checkout(&path, ref_)?;

      // open it and recursively clone its remotes
      Self::discover(&path, file)?.adopt(self).clone_all()?;
    }

    Ok(())
  }

  /// Clone a single remote
  pub fn clone_remote(&self, include: &Remote) -> Result<(), Error> {
    self.clone(
      &include.folder_name(),
      &include.url,
      &include.ref_,
      include.file.clone(),
    )
  }

  /// Recursively all Includes and Modules
  pub fn clone_all(&self) -> Result<(), Error> {
    for include in &self.data.includes {
      self.clone_remote(&include.remote)?;
    }

    Ok(())
  }

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

  /// Delete all cloned top-level targets
  pub fn clean_all(&self) -> Result<(), Error> {
    // no point in checking if it exists, because Mold::open creates it
    fs::remove_dir_all(&self.mold_dir)?;
    println!("{:>12} {}", "Deleted".red(), self.mold_dir.display());
    Ok(())
  }

  /// Merge every Include'd Mold into `self`
  pub fn process_includes(&mut self) -> Result<(), Error> {
    // merge all Includes into the current Mold. everything needs to be stuffed
    // into a vector because merging is a mutating action and `self` can't be
    // mutated while iterating through one of its fields.
    let mut others = vec![];
    for include in &self.data.includes {
      let path = self.mold_dir.join(include.remote.folder_name());
      let mut other = Self::discover(&path, include.remote.file.clone())?.adopt(self);

      // recursively merge
      other.process_includes()?;
      others.push((other, include.prefix.clone()));
    }

    for (data, prefix) in others {
      self.data.merge(data, &prefix);
    }

    Ok(())
  }

  /// Adopt any attributes from the parent that should be shared
  fn adopt(mut self, parent: &Self) -> Self {
    self.mold_dir = parent.mold_dir.clone();
    self.envs = parent.envs.clone();
    self
  }
}

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

impl Remote {
  /// Return this module's folder name in the format hash(url@ref)
  fn folder_name(&self) -> String {
    util::hash_url_ref(&self.url, &self.ref_)
  }
}

impl ToString for Remote {
  fn to_string(&self) -> String {
    if let Some(file) = &self.file {
      format!("{} @ {} /{}", self.url, self.ref_, file.display())
    } else {
      format!("{} @ {}", self.url, self.ref_)
    }
  }
}

impl Recipe {
  /// Figure out which command to run for our shell
  fn shell(&self, envs: &[String]) -> Result<String, Error> {
    match &self.command {
      Command::Shell(cmd) => return Ok(cmd.into()),
      Command::Map(map) => {
        for (test, cmd) in map {
          match serde::compile_expr(&test) {
            Ok(ex) => {
              if ex.apply(&envs) {
                return Ok(cmd.into());
              }
            }
            Err(err) => {
              println!("{}: '{}': {}", "Warning".bright_red(), test, err);
            }
          }
        }
      }
    }
    Err(failure::err_msg("Couldn't select command"))
  }

  /// Return this recipe's dependencies
  fn deps(&self) -> Vec<String> {
    self.deps.clone()
  }

  /// Return this recipe's help string
  fn help(&self) -> &str {
    &self.help
  }

  /// Return this recipe's working directory
  fn work_dir(&self) -> &Option<PathBuf> {
    &self.work_dir
  }
}

impl Task {
  /// Execute a recipe
  pub fn execute(self) -> Result<(), Error> {
    if self.args.is_empty() {
      return Err(failure::err_msg("empty command cannot be executed"));
    }

    let mut command = process::Command::new(&self.args[0]);
    command.args(&self.args[1..]);
    command.envs(self.vars);

    // FIXME this should be relative to root, no?
    if let Some(dir) = self.work_dir {
      command.current_dir(dir);
    }

    let exit_status = command.spawn().and_then(|mut handle| handle.wait())?;

    if !exit_status.success() {
      return Err(failure::err_msg("recipe returned non-zero exit status"));
    }

    Ok(())
  }

  pub fn args(&self) -> &Vec<String> {
    &self.args
  }
}

impl Remote {
  /// Parse a string into an Remote
  ///
  /// The format is roughly: url[#[ref][/file]], eg:
  ///   https://foo.com/mold.git -> ref = master, file = None
  ///   https://foo.com/mold.git#dev -> ref = dev, file = None
  ///   https://foo.com/mold.git#dev/dev.yaml, ref = dev, file = dev.yaml
  ///   https://foo.com/mold.git#/dev.yaml -> ref = master, file = dev.yaml
  fn parse(url: &str) -> Self {
    match url.find('#') {
      Some(idx) => {
        let (url, frag) = url.split_at(idx);
        let frag = frag.trim_start_matches('#');

        let (ref_, file) = match frag.find('/') {
          Some(idx) => {
            let (ref_, file) = frag.split_at(idx);
            let file = file.trim_start_matches('/');

            let ref_ = match ref_ {
              "" => "master".into(),
              _ => ref_.into(),
            };

            (ref_, Some(file.into()))
          }
          None => (frag.into(), None),
        };

        Self {
          url: url.into(),
          ref_,
          file,
        }
      }
      None => Self {
        url: url.into(),
        ref_: "master".into(),
        file: None,
      },
    }
  }
}

impl FromStr for Remote {
  type Err = Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Ok(Self::parse(s))
  }
}
