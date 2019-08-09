use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;

pub fn hash_url_ref(url: &str, ref_: &str) -> String {
  hash_string(&format!("{}@{}", url, ref_))
}

pub fn hash_string(string: &str) -> String {
  let mut hasher = DefaultHasher::new();
  string.hash(&mut hasher);
  format!("{:16x}", hasher.finish())
}
