use colored::*;
use failure::Error;
use std::io;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::time::Instant;

// This is a heavily modified version of the libgit2 "clone" example
// Its original form was public domain and linked to the CC0 Public Domain Dedication:
// <http://creativecommons.org/publicdomain/zero/1.0/>.

struct State<'a> {
  start: Instant,
  present: &'a str,
  past: &'a str,
  dots: usize,
  label: &'a str,
  cmd: Command,
}

impl<'a> State<'a> {
  fn new(present: &'a str, past: &'a str, label: &'a str) -> Self {
    let mut cmd = Command::new("git");
    cmd.stderr(Stdio::null()).stdout(Stdio::null());

    Self {
      start: Instant::now(),
      dots: 0,
      present,
      past,
      label,
      cmd,
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

pub fn clone(url: &str, path: &Path) -> Result<(), Error> {
  let label = format!("{} into {}", url, path.display());
  let mut state = State::new("     Cloning", "      Cloned", &label);

  state.cmd.arg("clone").arg(url).arg(path);

  state.wait()
}

pub fn ref_exists(path: &Path, ref_: &str) -> Result<bool, Error> {
  let exists = Command::new("git")
    .arg("rev-parse")
    .arg(ref_)
    .arg("--")
    .current_dir(path)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .and_then(|mut handle| handle.wait())?
    .success();

  Ok(exists)
}

pub fn checkout(path: &Path, ref_: &str) -> Result<(), Error> {
  let label = format!("{}", path.display());
  let mut state = State::new("    Fetching", "     Fetched", &label);
  state
    .cmd
    .arg("fetch")
    .arg("--all")
    .arg("--prune")
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
