use colored::*;
use failure::Error;
use itertools::Itertools;
use semver::Version;
use semver::VersionReq;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::hash::Hash;
use std::hash::Hasher;
use std::io::prelude::*;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::str::FromStr;
use std::string::ToString;

pub mod remote;

pub type RecipeMap = BTreeMap<String, Recipe>;
pub type IncludeVec = Vec<Include>;
pub type TypeMap = BTreeMap<String, Type>;
pub type VarMap = BTreeMap<String, String>; // TODO maybe down the line this should allow nulls to `unset` a variable
pub type EnvMap = BTreeMap<String, VarMap>;
pub type TaskSet = indexmap::IndexSet<String>;

const MOLD_FILES: &[&str] = &["mold.toml", "mold.yaml", "moldfile", "Moldfile"];

fn default_recipe_dir() -> PathBuf {
  "./mold".into()
}

fn default_git_ref() -> String {
  "master".into()
}

fn hash_url_ref(url: &str, ref_: &str) -> String {
  hash_string(&format!("{}@{}", url, ref_))
}

fn hash_string(string: &str) -> String {
  let mut hasher = DefaultHasher::new();
  string.hash(&mut hasher);
  format!("{:16x}", hasher.finish())
}

fn permutations(size: usize) -> Permutations {
  Permutations {
    idxs: (0..size).collect(),
    swaps: vec![0; size],
    i: 0,
  }
}

struct Permutations {
  idxs: Vec<usize>,
  swaps: Vec<usize>,
  i: usize,
}

impl Iterator for Permutations {
  type Item = Vec<usize>;

  fn next(&mut self) -> Option<Self::Item> {
    if self.i > 0 {
      loop {
        if self.i >= self.swaps.len() {
          return None;
        }
        if self.swaps[self.i] < self.i {
          break;
        }
        self.swaps[self.i] = 0;
        self.i += 1;
      }
      self.idxs.swap(self.i, (self.i & 1) * self.swaps[self.i]);
      self.swaps[self.i] += 1;
    }
    self.i = 1;
    Some(self.idxs.clone())
  }
}

fn apply<T: Clone>(idx: &[usize], to: &[T]) -> Vec<T> {
  idx.iter().map(|x| to[*x].clone()).collect()
}

fn all_permutations<T>(of: &[T]) -> Vec<Vec<&T>> {
  let mut result = vec![];

  for n in 1..=of.len() {
    for combo in of.iter().combinations(n) {
      let perms = permutations(combo.len());
      for perm in perms {
        result.push(apply(&perm, &combo));
      }
    }
  }

  result
}

#[derive(Debug)]
pub struct Mold {
  /// path to the moldfile
  file: PathBuf,

  /// path to the recipe scripts
  dir: PathBuf,

  /// (derived) path to the cloned repos
  clone_dir: PathBuf,

  /// (derived) path to the generated scripts
  script_dir: PathBuf,

  /// which environments to use in the environment
  envs: Vec<String>,

  /// the parsed moldfile data
  data: Moldfile,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Moldfile {
  /// Version of mold required to run this Moldfile
  pub version: Option<String>,

  /// The directory that recipe scripts can be found in
  #[serde(default = "default_recipe_dir")]
  pub recipe_dir: PathBuf,

  /// A map of includes
  #[serde(default)]
  pub includes: IncludeVec,

  /// A map of recipes
  #[serde(default)]
  pub recipes: RecipeMap,

  /// A map of interpreter types and characteristics
  #[serde(default)]
  pub types: TypeMap,

  /// A list of environment variables used to parametrize recipes
  ///
  /// BREAKING: Renamed from `environment` in 0.3.0
  #[serde(default)]
  pub variables: VarMap,

  /// A map of environment names to variable maps used to parametrize recipes
  ///
  /// ADDED: 0.3.0
  #[serde(default)]
  pub environments: EnvMap,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Include {
  /// Git URL of a remote repo
  pub url: String,

  /// Git ref to keep up with
  #[serde(alias = "ref", default = "default_git_ref")]
  pub ref_: String,

  /// Moldfile to look at
  pub file: Option<PathBuf>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct RecipeBase {
  /// A short description of the group's contents
  #[serde(default)]
  pub help: String,

  /// A list of environment variables that overrides the base environment
  ///
  /// BREAKING: Renamed from `environment` in 0.3.0
  #[serde(default)]
  pub variables: VarMap,

  /// A map of environment names to variable maps used to parametrize recipes
  ///
  /// ADDED: 0.3.0
  #[serde(default)]
  pub environments: EnvMap,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Recipe {
  // apparently the order here matters?
  Group(Group),
  Script(Script),
  File(File),
  Command(Command),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Group {
  /// Base data
  #[serde(flatten)]
  pub base: RecipeBase,

  /// Git URL of a remote repo
  pub url: String,

  /// Git ref to keep up with
  #[serde(alias = "ref", default = "default_git_ref")]
  pub ref_: String,

  /// Moldfile to look at
  pub file: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct File {
  /// Base data
  #[serde(flatten)]
  pub base: RecipeBase,

  /// A list of pre-execution dependencies
  #[serde(default)]
  pub deps: Vec<String>,

  /// The actual root of this script
  ///
  /// This is used for Includes, where the command may be lifted up to the
  /// top-level, but the root is located in a different location
  #[serde(skip)]
  pub root: Option<PathBuf>,

  /// Which interpreter should be used to execute this script
  #[serde(alias = "type")]
  pub type_: String,

  /// The script file name
  ///
  /// If left undefined, Mold will attempt to discover the recipe name by
  /// searching the recipe_dir for any files that start with the recipe name and
  /// have an appropriate extension for the specified interpreter type.
  pub file: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Script {
  /// Base data
  #[serde(flatten)]
  pub base: RecipeBase,

  /// A list of pre-execution dependencies
  #[serde(default)]
  pub deps: Vec<String>,

  /// Which interpreter should be used to execute this script
  #[serde(alias = "type")]
  pub type_: String,

  /// The script contents as a multiline string
  pub script: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Command {
  /// Base data
  #[serde(flatten)]
  pub base: RecipeBase,

  /// A list of pre-execution dependencies
  #[serde(default)]
  pub deps: Vec<String>,

  /// A list of command arguments
  #[serde(default)]
  pub command: Vec<String>,
}

#[derive(Debug)]
pub struct Task {
  args: Vec<String>,
  vars: Option<VarMap>,
}

impl Mold {
  /// Open a moldfile and load it
  pub fn open(path: &Path) -> Result<Mold, Error> {
    let mut file = fs::File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let data: Moldfile = match path.extension().and_then(OsStr::to_str) {
      Some("yaml") | Some("yml") => serde_yaml::from_str(&contents)?,
      _ => toml::de::from_str(&contents)?,
    };

    let self_version = Version::parse(clap::crate_version!())?;
    let target_version = match data.version {
      Some(ref version) => VersionReq::parse(&version)?,
      None => VersionReq::parse(clap::crate_version!())?,
    };

    if !target_version.matches(&self_version) {
      return Err(failure::format_err!(
        "Incompatible versions: file {} requires version {}, but current version is {}",
        path.to_str().unwrap().blue(),
        target_version.to_string().green(),
        self_version.to_string().red()
      ));
    }

    let dir = path.with_file_name(&data.recipe_dir);
    let clone_dir = dir.join(".clones");
    let script_dir = dir.join(".scripts");

    if !dir.is_dir() {
      fs::create_dir(&dir)?;
    }
    if !clone_dir.is_dir() {
      fs::create_dir(&clone_dir)?;
    }
    if !script_dir.is_dir() {
      fs::create_dir(&script_dir)?;
    }

    Ok(Mold {
      file: fs::canonicalize(path)?,
      dir: fs::canonicalize(dir)?,
      clone_dir: fs::canonicalize(clone_dir)?,
      script_dir: fs::canonicalize(script_dir)?,
      envs: vec![],
      data,
    })
  }

  /// Try to locate a moldfile by walking up the directory tree
  fn locate_file(name: &Path) -> Result<PathBuf, Error> {
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

  /// Try to locate and open a moldfile by directory
  ///
  /// Checks for MOLD_FILES
  fn discover_dir(name: &Path) -> Result<Mold, Error> {
    let path = MOLD_FILES
      .iter()
      .find_map(|file| Self::locate_file(&name.join(file)).ok())
      .ok_or_else(|| {
        failure::format_err!(
          "Cannot locate moldfile, tried the following:\n{}",
          MOLD_FILES.join(" ").red()
        )
      })?;
    Self::open(&path)
  }

  /// Try to locate and open a moldfile by name
  fn discover_file(name: &Path) -> Result<Mold, Error> {
    let path = Self::locate_file(name)?;
    Self::open(&path)
  }

  /// Try to locate a file or a directory
  pub fn discover(dir: &Path, file: Option<PathBuf>) -> Result<Mold, Error> {
    // I think this should take Option<&Path> but I couldn't figure out how to
    // please the compiler when I have an existing Option<PathBuf>, so...  I'm
    // just using .clone() on it.
    match file {
      Some(file) => Self::discover_file(&dir.join(file)),
      None => Self::discover_dir(dir),
    }
  }

  /// Generate a list of all permutations of the activated environments
  ///
  /// Environments are yielded as strings joined by plus signs.
  /// eg, given environments {a, b, c}, this will yield:
  ///    a, b, c, a+b, b+a, a+c, c+a, b+c, c+b, etc...
  fn cross_envs(&self) -> Vec<String> {
    let result = all_permutations(&self.envs);
    result.iter().map(|x| x.iter().join("+")).collect()
  }

  /// Return this moldfile's variables with activated environments
  pub fn env_vars(&self) -> VarMap {
    let mut vars = self.data.variables.clone();
    for env_name in self.cross_envs() {
      if let Some(env) = self.data.environments.get(&env_name) {
        vars.extend(env.iter().map(|(k, v)| (k.clone(), v.clone())));
      }
    }
    vars
  }

  pub fn set_env(&mut self, env: Option<String>) {
    self.envs = match env {
      Some(envs) => envs.split(',').map(|x| x.into()).collect(),
      None => vec![],
    };
  }

  /// Find a Recipe by name
  fn find_recipe(&self, target_name: &str) -> Result<&Recipe, Error> {
    self
      .data
      .recipes
      .get(target_name)
      .ok_or_else(|| failure::format_err!("Couldn't locate target '{}'", target_name.red()))
  }

  /// Find a Type by name
  fn find_type(&self, type_name: &str) -> Result<&Type, Error> {
    self
      .data
      .types
      .get(type_name)
      .ok_or_else(|| failure::format_err!("Couldn't locate type '{}'", type_name.red()))
  }

  /// Find a Recipe by name and attempt to unwrap it to a Group
  fn find_group(&self, group_name: &str) -> Result<&Group, Error> {
    // unwrap the group or quit
    match self.find_recipe(group_name)? {
      Recipe::Command(_) => Err(failure::err_msg("Requested recipe is a command")),
      Recipe::File(_) => Err(failure::err_msg("Requested recipe is a file")),
      Recipe::Group(target) => Ok(target),
      Recipe::Script(_) => Err(failure::err_msg("Requested recipe is a script")),
    }
  }

  fn open_group(&self, group_name: &str) -> Result<Mold, Error> {
    let target = self.find_group(group_name)?;
    let path = self.clone_dir.join(&target.folder_name());
    let mut mold = Self::discover(&path, target.file.clone())?.adopt(self);
    mold.process_includes()?;
    Ok(mold)
  }

  fn open_include(&self, target: &Include) -> Result<Mold, Error> {
    let path = self.clone_dir.join(&target.folder_name());
    let mold = Self::discover(&path, target.file.clone())?.adopt(self);
    Ok(mold)
  }

  /// Recursively fetch/checkout for all groups that have already been cloned
  pub fn update_all(&self) -> Result<(), Error> {
    self.update_all_track(&mut HashSet::new())
  }

  /// Recursively fetch/checkout for all groups that have already been cloned,
  /// but with extra checks to avoid infinite recursion cycles
  fn update_all_track(&self, updated: &mut HashSet<PathBuf>) -> Result<(), Error> {
    // `updated` contains all of the directories that have been, well, updated.
    // it *needs* to be passed to recursive calls.

    // both loops iterate through their respective items:
    // * find the expected path
    // * make sure it exists (ie, is cloned) and hasn't been visited
    // * track it as visited
    // * fetch / checkout
    // * recurse into it

    // find all groups that have already been cloned and update them
    for (name, recipe) in &self.data.recipes {
      if let Recipe::Group(group) = recipe {
        let path = self.clone_dir.join(group.folder_name());
        if path.is_dir() && !updated.contains(&path) {
          updated.insert(path.clone());
          remote::checkout(&path, &group.ref_)?;
          self.open_group(name)?.update_all_track(updated)?;
        }
      }
    }

    // find all Includes that have already been cloned and update them
    for include in &self.data.includes {
      let path = self.clone_dir.join(include.folder_name());
      if path.is_dir() && !updated.contains(&path) {
        updated.insert(path.clone());
        remote::checkout(&path, &include.ref_)?;
        self.open_include(&include)?.update_all_track(updated)?;
      }
    }

    Ok(())
  }

  /// Recursively all Includes and Groups
  pub fn clone_all(&self) -> Result<(), Error> {
    for recipe in self.data.recipes.values() {
      if let Recipe::Group(group) = recipe {
        self.clone_group(&group)?;
      }
    }

    for include in &self.data.includes {
      self.clone_include(&include)?;
    }

    Ok(())
  }

  /// Clone a single remote group
  fn clone_group(&self, group: &Group) -> Result<(), Error> {
    self.clone(
      &group.folder_name(),
      &group.url,
      &group.ref_,
      group.file.clone(),
    )
  }

  /// Clone a single remote include
  pub fn clone_include(&self, include: &Include) -> Result<(), Error> {
    self.clone(
      &include.folder_name(),
      &include.url,
      &include.ref_,
      include.file.clone(),
    )
  }

  /// Clone a single remote reference and then recursively clone subremotes
  fn clone(
    &self,
    folder_name: &str,
    url: &str,
    ref_: &str,
    file: Option<PathBuf>,
  ) -> Result<(), Error> {
    let path = self.clone_dir.join(folder_name);
    if !path.is_dir() {
      remote::clone(url, &path)?;
      remote::checkout(&path, ref_)?;

      // open it and recursively clone + merge
      Self::discover(&path, file.clone())?
        .adopt(self)
        .clone_all()?;
    }

    Ok(())
  }

  /// Delete all cloned top-level targets
  pub fn clean_all(&self) -> Result<(), Error> {
    // no point in checking if it exists, because Mold::open creates it
    fs::remove_dir_all(&self.clone_dir)?;
    println!("{:>12} {}", "Deleted".red(), self.clone_dir.display());

    fs::remove_dir_all(&self.script_dir)?;
    println!("{:>12} {}", "Deleted".red(), self.script_dir.display());
    Ok(())
  }

  /// Find all dependencies for a given *set* of tasks
  pub fn find_all_dependencies(&self, targets: &TaskSet) -> Result<TaskSet, Error> {
    let mut new_targets = TaskSet::new();

    for target_name in targets {
      new_targets.extend(self.find_task_dependencies(target_name)?);
      new_targets.insert(target_name.clone());
    }

    Ok(new_targets)
  }

  /// Find all dependencies for a *single* task
  fn find_task_dependencies(&self, target: &str) -> Result<TaskSet, Error> {
    // check if this is a nested subrecipe that we'll have to recurse into
    if target.contains('/') {
      let splits: Vec<_> = target.splitn(2, '/').collect();
      let group_name = splits[0];
      let recipe_name = splits[1];

      let group = self.open_group(group_name)?;
      let deps = group.find_task_dependencies(recipe_name)?;
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
    let deps = recipe.deps().iter().map(ToString::to_string).collect();
    self.find_all_dependencies(&deps)
  }

  /// Find a Task object for a given recipe name
  ///
  /// This entails recursing through various groups to find the the appropriate
  /// Task.
  pub fn find_task(&self, target_name: &str, prev_vars: &VarMap) -> Result<Option<Task>, Error> {
    // check if we're executing a nested subrecipe that we'll have to recurse into
    if target_name.contains('/') {
      let splits: Vec<_> = target_name.splitn(2, '/').collect();
      let group_name = splits[0];
      let recipe_name = splits[1];
      let recipe = self.find_recipe(group_name)?;
      let group = self.open_group(group_name)?;

      // merge this moldfile's variables with its parent.
      // the parent has priority and overrides this moldfile because it's called recursively:
      //   $ mold foo/bar/baz
      // will call bar/baz with foo as the parent, which will call baz with bar as
      // the parent. we want foo's moldfile to override bar's moldfile to override
      // baz's moldfile, because baz should be the least specialized.
      let mut vars = group.env_vars().clone();
      vars.extend(prev_vars.iter().map(|(k, v)| (k.clone(), v.clone())));

      let mut task = group.find_task(recipe_name, &vars)?;
      if let Some(task) = &mut task {
        // not sure if this is the right ordering to update variables in, but
        // it's done here so that parent group's configuration can override one
        // of the subrecipes in the group
        if let Some(vars) = &mut task.vars {
          vars.extend(
            recipe
              .env_vars(&self.cross_envs())
              .iter()
              .map(|(k, v)| (k.clone(), v.clone())),
          );
        }
      }

      return Ok(task);
    }

    // ...not executing subrecipe, so look up the top-level recipe
    let recipe = self.find_recipe(target_name)?;

    // extend the variables with the recipe's variables
    let mut vars = prev_vars.clone();
    vars.extend(
      recipe
        .env_vars(&self.cross_envs())
        .iter()
        .map(|(k, v)| (k.clone(), v.clone())),
    );

    let task = match recipe {
      Recipe::Command(target) => Some(Task::from_args(&target.command, Some(&vars))),
      Recipe::File(target) => {
        // what the interpreter is for this recipe
        let type_ = self.find_type(&target.type_)?;

        // use the target's root, but fall back to our own
        // (feels like I shouldn't have to clone these, though...)
        let search_dir = target.root.clone().unwrap_or_else(|| self.dir.clone());

        // find the script file to execute
        let script = match &target.file {
          Some(x) => search_dir.join(x),

          // we need to look it up based on our interpreter's known extensions
          None => type_.find(&search_dir, &target_name)?,
        };

        Some(type_.task(&script.to_str().unwrap(), &vars))
      }
      Recipe::Script(target) => {
        // what the interpreter is for this recipe
        let type_ = self.find_type(&target.type_)?;

        // locate a file to write the script to
        let mut temp_file = self.script_dir.join(hash_string(&target.script));
        if let Some(x) = type_.extensions.get(0) {
          temp_file.set_extension(&x);
        }

        fs::write(&temp_file, &target.script)?;

        Some(type_.task(&temp_file.to_str().unwrap(), &vars))
      }
      Recipe::Group(_) => {
        // this is kinda hacky, but... whatever. it should probably
        // somehow map into a Task instead, but this is good enough.
        let group_name = format!("{}/", target_name);
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
  fn help_prefixed(&self, prefix: &str) -> Result<(), Error> {
    for (name, recipe) in &self.data.recipes {
      let colored_name = match recipe {
        Recipe::Command(_) => name.yellow(),
        Recipe::File(_) => name.cyan(),
        Recipe::Group(_) => format!("{}/", name).magenta(),
        Recipe::Script(_) => name.yellow(),
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
    }

    Ok(())
  }

  /// Merge every Include'd Mold into `self`
  pub fn process_includes(&mut self) -> Result<(), Error> {
    // merge all Includes into the current Mold. everything needs to be stuffed
    // into a vector because merging is a mutating action and `self` can't be
    // mutated while iterating through one of its fields.
    let mut merges = vec![];
    for include in &self.data.includes {
      let path = self.clone_dir.join(include.folder_name());
      let mut merge = Self::discover(&path, include.file.clone())?.adopt(self);

      // recursively merge
      merge.process_includes()?;
      merges.push(merge);
    }

    for merge in merges {
      self.data.merge_absent(merge);
    }

    Ok(())
  }

  /// Merge a single Include into `self`
  pub fn process_include(&mut self, include: &Include) -> Result<(), Error> {
    let path = self.clone_dir.join(include.folder_name());
    let mut merge = Self::discover(&path, include.file.clone())?.adopt(self);

    // recursively merge
    merge.process_includes()?;
    self.data.merge_absent(merge);
    Ok(())
  }

  /// Adopt any attributes from the parent that should be shared
  fn adopt(mut self, parent: &Self) -> Self {
    self.clone_dir = parent.clone_dir.clone();
    self.script_dir = parent.script_dir.clone();
    self.envs = parent.envs.clone();
    self
  }
}

impl Moldfile {
  /// Merges any types in other missing in self
  pub fn merge_absent(&mut self, other: Mold) {
    for (type_name, type_) in other.data.types {
      self.types.entry(type_name).or_insert(type_);
    }

    for (recipe_name, mut recipe) in other.data.recipes {
      recipe.set_root(Some(other.dir.clone()));
      self.recipes.entry(recipe_name).or_insert(recipe);
    }
  }
}

impl Include {
  /// Return this group's folder name in the format hash(url@ref)
  fn folder_name(&self) -> String {
    hash_url_ref(&self.url, &self.ref_)
  }

  /// Parse a string into an Include
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
              "" => default_git_ref(),
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
        ref_: default_git_ref(),
        file: None,
      },
    }
  }
}

impl FromStr for Include {
  type Err = Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Ok(Self::parse(s))
  }
}

impl Type {
  /// Create a Task ready to execute a script
  fn task(&self, script: &str, vars: &VarMap) -> Task {
    let args: Vec<_> = self
      .command
      .iter()
      .map(|x| if x == "?" { script.into() } else { x.clone() })
      .collect();

    Task {
      args,
      vars: Some(vars.clone()),
    }
  }

  /// Attempt to discover an appropriate script in a recipe directory
  fn find(&self, dir: &Path, name: &str) -> Result<PathBuf, Error> {
    // set up the pathbuf to look for dir/name
    let mut path = dir.join(name);

    // try all of our known extensions, early returning on the first match
    for ext in &self.extensions {
      path.set_extension(ext);
      if path.is_file() {
        return Ok(path);
      }
    }

    // support no ext
    if path.is_file() {
      return Ok(path);
    }

    Err(failure::err_msg("Couldn't find a file"))
  }
}

impl Recipe {
  /// Return this recipe's dependencies
  fn deps(&self) -> Vec<String> {
    match self {
      Recipe::File(s) => s.deps.clone(),
      Recipe::Command(c) => c.deps.clone(),
      _ => vec![],
    }
  }

  /// Return this recipe's help string
  fn help(&self) -> &str {
    match self {
      Recipe::Command(c) => &c.base.help,
      Recipe::File(f) => &f.base.help,
      Recipe::Group(g) => &g.base.help,
      Recipe::Script(s) => &s.base.help,
    }
  }

  /// Return this recipe's variables
  fn vars(&self) -> &VarMap {
    match self {
      Recipe::File(f) => &f.base.variables,
      Recipe::Command(c) => &c.base.variables,
      Recipe::Script(s) => &s.base.variables,
      Recipe::Group(g) => &g.base.variables,
    }
  }

  /// Return this recipe's environments
  fn envs(&self) -> &EnvMap {
    match self {
      Recipe::File(f) => &f.base.environments,
      Recipe::Command(c) => &c.base.environments,
      Recipe::Script(s) => &s.base.environments,
      Recipe::Group(g) => &g.base.environments,
    }
  }

  /// Return this recipe's variables with activated environments
  pub fn env_vars(&self, envs: &[String]) -> VarMap {
    let mut vars = self.vars().clone();
    let env_maps = self.envs();
    for env_name in envs {
      if let Some(env) = env_maps.get(env_name) {
        vars.extend(env.iter().map(|(k, v)| (k.clone(), v.clone())));
      }
    }
    vars
  }

  /// Set this recipe's root
  fn set_root(&mut self, to: Option<PathBuf>) {
    if let Recipe::File(s) = self {
      s.root = to;
    }
  }
}

impl Group {
  /// Return this group's folder name in the format hash(url@ref)
  fn folder_name(&self) -> String {
    hash_url_ref(&self.url, &self.ref_)
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

    if let Some(vars) = &self.vars {
      command.envs(vars);
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
  pub fn print_vars(&self) {
    if self.args.is_empty() {
      return;
    }

    if let Some(vars) = &self.vars {
      for (name, value) in vars {
        println!("  {} = \"{}\"", format!("${}", name).bright_cyan(), value);
      }
    }
  }

  /// Create a Task from a Vec of strings
  fn from_args(args: &[String], vars: Option<&VarMap>) -> Task {
    Task {
      args: args.into(),
      vars: vars.map(std::clone::Clone::clone),
    }
  }
}
