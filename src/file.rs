/// This module contains all the associated structs for serializing and
/// deserializing a moldfile.
use indexmap::IndexMap;
use indexmap::IndexSet;
use std::collections::BTreeMap;
use std::path::PathBuf;

// maps sorted by insertion order
pub type IncludeVec = Vec<Include>;
pub type TargetSet = IndexSet<String>;
pub type VarMap = IndexMap<String, String>; // TODO maybe down the line this should allow nulls to `unset` a variable
pub type EnvMap = IndexMap<String, VarMap>;
pub type CommandMap = IndexMap<String, String>;

// maps sorted alphabetically
pub type RecipeMap = BTreeMap<String, Recipe>;

pub const DEFAULT_FILES: &[&str] = &["mold.yaml", "mold.yml", "moldfile", "Moldfile"];

#[derive(Debug)]
pub struct Moldfile {
  /// Version of mold required to run this Moldfile
  pub version: String,

  /// Simple help string
  pub help: Option<String>,

  /// A map of includes
  pub includes: IncludeVec,

  /// A map of recipes
  pub recipes: RecipeMap,

  /// A map of environment variables
  pub variables: VarMap,

  /// A list of conditionals
  pub environments: EnvMap,
}

#[derive(Debug, Clone)]
pub struct Remote {
  /// Git URL of a remote repo
  pub url: String,

  /// Git ref to keep up with
  pub ref_: String,

  /// Moldfile to look at
  pub file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Include {
  /// Remote to include
  pub remote: Remote,

  /// Prefix to prepend
  pub prefix: String,
}

#[derive(Debug, Clone)]
pub enum Command {
  Shell(String),
  Map(CommandMap),
}

#[derive(Debug, Clone)]
pub struct Recipe {
  /// A short description of the module's contents
  pub help: String,

  /// The working directory relative to the calling Moldfile's root_dir
  pub work_dir: Option<PathBuf>,

  /// A list of prerequisites
  pub deps: Vec<String>,

  /// The command to execute
  pub command: Command,

  /// The script contents as a multiline string
  ///
  /// Its contents will be written to a file pointed to by $MOLD_SCRIPT
  pub script: Option<String>,
}
