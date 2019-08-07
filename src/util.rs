use itertools::Itertools;
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

struct Permutations {
  idxs: Vec<usize>,
  swaps: Vec<usize>,
  i: usize,
}

impl Permutations {
  pub fn new(size: usize) -> Self {
    Self {
      idxs: (0..size).collect(),
      swaps: vec![0; size],
      i: 0,
    }
  }
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

fn apply_permutation<T: Clone>(idx: &[usize], to: &[T]) -> Vec<T> {
  idx.iter().map(|x| to[*x].clone()).collect()
}

pub fn all_permutations<T>(of: &[T]) -> Vec<Vec<&T>> {
  let mut result = vec![];

  for n in 1..=of.len() {
    for combo in of.iter().combinations(n) {
      let perms = Permutations::new(combo.len());
      for perm in perms {
        result.push(apply_permutation(&perm, &combo));
      }
    }
  }

  result
}
