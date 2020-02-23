use super::util;
use colored::*;
use failure::Error;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;
use std::string::ToString;
use std::time::Instant;

struct State<'a> {
  start: Instant,
  present: &'a str,
  past: &'a str,
  dots: usize,
  label: &'a str,
  cmd: Command,
}

fn new_cmd() -> Command {
  let mut cmd = Command::new("git");
  cmd.stderr(Stdio::null()).stdout(Stdio::null());
  cmd
}

impl<'a> State<'a> {
  fn new(present: &'a str, past: &'a str, label: &'a str) -> Self {
    Self {
      start: Instant::now(),
      dots: 0,
      present,
      past,
      label,
      cmd: new_cmd(),
    }
  }

  fn print_progress(&mut self) {
    let duration = (Instant::now() - self.start).as_millis() as usize;
    let dotlist = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    if duration > 33 {
      self.start = Instant::now();
      self.dots = (self.dots + 1) % dotlist.len();
      print!(
        "{} {}... {}\r",
        self.present.yellow(),
        self.label,
        dotlist[self.dots]
      );
      io::stdout().flush().unwrap();
    }
  }

  fn print_done(&self) {
    println!("{} {}     ", self.past.green(), self.label);
    io::stdout().flush().unwrap();
  }

  fn wait(&mut self) -> Result<(), Error> {
    let mut child = self.cmd.spawn()?;
    loop {
      if child.try_wait()?.is_some() {
        self.print_done();
        break;
      }
      self.print_progress();
    }
    Ok(())
  }
}

fn pull(url: &str, path: &Path) -> Result<(), Error> {
  let label = format!("{} into {}", url, path.display());
  let mut state = State::new("     Cloning", "      Cloned", &label);

  state.cmd.arg("clone").arg(url).arg(path);

  state.wait()
}

fn ref_exists(path: &Path, ref_: &str) -> Result<bool, Error> {
  let exists = new_cmd()
    .arg("rev-parse")
    .arg(ref_)
    .arg("--")
    .current_dir(path)
    .spawn()
    .and_then(|mut handle| handle.wait())?
    .success();

  Ok(exists)
}

fn checkout(path: &Path, ref_: &str) -> Result<(), Error> {
  let label = format!("{}", path.display());
  let mut state = State::new("    Fetching", "     Fetched", &label);
  state
    .cmd
    .args(&["fetch", "--all", "--prune"])
    .current_dir(path);

  state.wait()?;

  let refs = vec![format!("tags/{}", ref_), format!("origin/{}", ref_)];

  for target in refs {
    if ref_exists(path, &target)? {
      let label = format!("{} into {}", path.display(), ref_);
      let mut state = State::new("   Switching", "    Switched", &label);
      state.cmd.arg("checkout").arg(target).current_dir(path);

      return state.wait();
    }
  }

  Err(failure::format_err!("Couldn't locate ref '{}'", ref_.red()))
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

impl Remote {
  /// Return this module's folder name in the format hash(url@ref)
  fn folder_name(&self) -> String {
    util::hash_url_ref(&self.url, &self.ref_)
  }

  pub fn path(&self, mold_dir: &Path) -> PathBuf {
    mold_dir.join(self.folder_name())
  }

  pub fn exists(&self, mold_dir: &Path) -> bool {
    self.path(mold_dir).is_dir()
  }

  pub fn pull(&self, mold_dir: &Path) -> Result<(), Error> {
    let path = self.path(mold_dir);
    pull(&format!("https://{}", self.url), &path).or_else(|_| pull(&self.url, &path))
  }

  pub fn checkout(&self, mold_dir: &Path) -> Result<(), Error> {
    let path = self.path(mold_dir);
    checkout(&path, &self.ref_)
  }

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

impl ToString for Remote {
  fn to_string(&self) -> String {
    if let Some(file) = &self.file {
      format!("{}#{}/{}", self.url, self.ref_, file.display())
    } else {
      format!("{}#{}", self.url, self.ref_)
    }
  }
}

impl FromStr for Remote {
  type Err = Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Ok(Self::parse(s))
  }
}
