/*
 * libgit2 "clone" example
 *
 * Written by the libgit2 contributors
 *
 * To the extent possible under law, the author(s) have dedicated all copyright
 * and related and neighboring rights to this software to the public domain
 * worldwide. This software is distributed without any warranty.
 *
 * You should have received a copy of the CC0 Public Domain Dedication along
 * with this software. If not, see
 * <http://creativecommons.org/publicdomain/zero/1.0/>.
 */

use colored::*;
use failure::Error;
use git2::build::CheckoutBuilder;
use git2::build::RepoBuilder;
use git2::FetchOptions;
use git2::RemoteCallbacks;
use git2::Repository;
use std::cell::RefCell;
use std::io;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

struct State<'a> {
  start: Instant,
  present: &'a str,
  past: &'a str,
  dots: usize,
  label: &'a str,
}

fn print_progress(state: &mut State) {
  let duration = (Instant::now() - state.start).as_millis() as usize;
  let dotlist = [
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
  ];
  if duration > 33 {
    state.start = Instant::now();
    state.dots = (state.dots + 1) % dotlist.len();
    print!(
      "{} {}... {}\r",
      state.present.yellow(),
      state.label,
      dotlist[state.dots]
    );
    io::stdout().flush().unwrap();
  }
}

fn print_done(state: &mut State) {
  println!("{} {}     ", state.past.green(), state.label);
  io::stdout().flush().unwrap();
}

pub fn clone(url: &str, path: &Path) -> Result<(), Error> {
  let label = format!("{} into {}", url, path.display());

  let state = RefCell::new(State {
    start: Instant::now(),
    present: "     Cloning",
    past: "      Cloned",
    label: &label,
    dots: 0,
  });

  let mut cb = RemoteCallbacks::new();
  cb.transfer_progress(|_| {
    let mut state = state.borrow_mut();
    print_progress(&mut *state);
    true
  });

  let mut co = CheckoutBuilder::new();
  co.progress(|_, _, _| {
    let mut state = state.borrow_mut();
    print_progress(&mut *state);
  });

  let mut fo = FetchOptions::new();
  fo.remote_callbacks(cb);
  RepoBuilder::new()
    .fetch_options(fo)
    .with_checkout(co)
    .clone(url, path)?;;

  print_done(&mut state.borrow_mut());

  Ok(())
}

pub fn checkout(path: &Path, ref_: &str) -> Result<(), Error> {
  let repo = Repository::discover(path)?;
  let mut remote = repo.find_remote("origin")?;

  let label = &format!("{} to {}", path.display(), ref_);
  let state = RefCell::new(State {
    start: Instant::now(),
    present: "    Updating",
    past: "     Updated",
    label: &label,
    dots: 0,
  });

  let mut cb = RemoteCallbacks::new();
  cb.transfer_progress(|_| {
    let mut state = state.borrow_mut();
    print_progress(&mut *state);
    true
  });

  let mut fo = FetchOptions::new();
  fo.remote_callbacks(cb);
  remote.fetch(&[ref_], Some(&mut fo), None)?;

  let name = String::from("refs/remotes/origin/") + ref_;
  repo.set_head(&name)?;

  print_done(&mut state.borrow_mut());

  Ok(())
}
