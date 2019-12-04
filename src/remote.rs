use colored::*;
use failure::Error;
use git2::build::CheckoutBuilder;
use git2::build::RepoBuilder;
use git2::Cred;
use git2::CredentialType;
use git2::FetchOptions;
use git2::RemoteCallbacks;
use git2::Repository;
use std::cell::RefCell;
use std::io;
use std::io::Write;
use std::path::Path;
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
}

fn print_progress(state: &mut State) {
  let duration = (Instant::now() - state.start).as_millis() as usize;
  let dotlist = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
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

pub fn git_credentials_callback(
  _user: &str,
  _user_from_url: Option<&str>,
  _cred: CredentialType,
) -> Result<Cred, git2::Error> {
  if let Some(home_dir) = dirs::home_dir() {
    let pub_key = home_dir.join(".ssh/id_rsa.pub");
    let priv_key = home_dir.join(".ssh/id_rsa");
    let credentials = Cred::ssh_key("git", Some(&pub_key), &priv_key, None)
      .expect("Could not create credentials object");

    Ok(credentials)
  } else {
    Err(git2::Error::from_str("Couldn't locate home directory"))
  }
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

  let mut callbacks = RemoteCallbacks::new();
  callbacks.transfer_progress(|_| {
    let mut state = state.borrow_mut();
    print_progress(&mut *state);
    true
  });
  callbacks.credentials(git_credentials_callback);

  let mut fetch = FetchOptions::new();
  fetch.remote_callbacks(callbacks);

  let mut builder = CheckoutBuilder::new();
  builder.progress(|_, _, _| {
    let mut state = state.borrow_mut();
    print_progress(&mut *state);
  });

  RepoBuilder::new()
    .fetch_options(fetch)
    .with_checkout(builder)
    .clone(url, path)?;

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

  let mut callbacks = RemoteCallbacks::new();
  callbacks.transfer_progress(|_| {
    let mut state = state.borrow_mut();
    print_progress(&mut *state);
    true
  });
  callbacks.credentials(git_credentials_callback);

  let mut fetch = FetchOptions::new();
  fetch.remote_callbacks(callbacks);

  remote.fetch(&[ref_], Some(&mut fetch), None)?;

  let tag_name = format!("tags/{}", ref_);
  let branch_name = format!("origin/{}", ref_);
  let object = repo
    .revparse_single(&tag_name)
    .or_else(|_| repo.revparse_single(&branch_name))?;
  repo.set_head_detached(object.id())?;

  let mut checkout = CheckoutBuilder::new();
  checkout.force();
  repo.checkout_head(Some(&mut checkout))?;

  print_done(&mut state.borrow_mut());

  Ok(())
}
