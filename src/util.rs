use failure::Error;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::PathBuf;
use std::process;
use which::which;

pub fn hash_url_ref(url: &str, ref_: &str) -> String {
  hash_string(&format!("{}@{}", url, ref_))
}

pub fn hash_string(string: &str) -> String {
  let mut hasher = DefaultHasher::new();
  string.hash(&mut hasher);
  format!("{:016x}", hasher.finish())
}

pub fn cat(path: PathBuf) -> Result<(), Error> {
  let bin = which("bat").or_else(|_| which("cat"))?;
  let mut command = process::Command::new(bin);
  command.args(vec![path]);

  let exit_status = command.spawn().and_then(|mut handle| handle.wait())?;
  if !exit_status.success() {
    return Err(failure::err_msg("bat returned non-zero exit status"));
  }

  Ok(())
}
