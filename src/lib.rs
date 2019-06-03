use failure::Error;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::exit;
use std::process::Command;

pub type RecipeSet = BTreeMap<String, Recipe>;
pub type TypeSet = BTreeMap<String, Type>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Moldfile {
  #[serde(default = "default_recipe_dir")]
  pub recipe_dir: String,

  pub recipes: RecipeSet,

  pub types: TypeSet,
}

fn default_recipe_dir() -> String {
  "./recipes".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Recipe {
  #[serde(alias = "type")]
  pub type_: String,
  pub help: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Type {
  pub command: Vec<String>,

  #[serde(default = "default_extensions")]
  pub extensions: Vec<String>,
}

impl Type {
  pub fn exec(&self, path: &Path) -> Result<(), Error> {
    let mut args = self.command.clone();
    let command = args.remove(0);
    let args: Vec<_> = args
      .iter()
      .map(|x| if x == "?" { path.to_str().unwrap() } else { x })
      .collect();

    let exit_status = Command::new(&command)
      .args(&args[..])
      .spawn()
      .and_then(|mut handle| handle.wait())?;

    if !exit_status.success() {
      return Err(failure::err_msg("recipe exited with non-zero code"));
    }

    Ok(())
  }

  pub fn find(&self, dir: &Path, name: &str) -> Result<PathBuf, Error> {
    let mut pb = dir.to_path_buf();
    pb.push(name);
    for ext in &self.extensions {
      pb.set_extension(ext);
      if pb.is_file() {
        return Ok(pb);
      }
    }
    Err(failure::err_msg("Couldn't find a file"))
  }
}

fn default_extensions() -> Vec<String> {
  return vec![];
}
