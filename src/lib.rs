use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::collections::BTreeMap;

pub type RecipeSet = BTreeMap<String, Recipe>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Moldfile {
  pub recipe_dir: Option<String>,
  pub recipes: RecipeSet,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Recipe {
  pub interpreter: Option<String>,
  pub help: Option<String>,
}
