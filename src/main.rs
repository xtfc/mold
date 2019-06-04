use exitfailure::ExitFailure;
use mold::remote;
use mold::Moldfile;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::path::PathBuf;
use structopt::StructOpt;

/// A new front-end for Git
#[derive(StructOpt, Debug)]
#[structopt(raw(setting = "structopt::clap::AppSettings::ColoredHelp"))]
pub struct Root {
  /// Path to the moldfile
  #[structopt(long = "file", short = "f", default_value = "moldfile")]
  pub file: std::path::PathBuf,

  /// Don't print extraneous information
  #[structopt(long = "quiet", short = "q")]
  pub quiet: bool,

  /// dbg! the parsed moldfile
  #[structopt(long = "debug", short = "d")]
  pub debug: bool,

  #[structopt(long = "update", short = "u")]
  pub update: bool,

  /// Which recipe to run
  pub target: Option<String>,
}

fn main() -> Result<(), ExitFailure> {
  let args = Root::from_args();
  env_logger::init();

  // read and deserialize the moldfile
  let mut file = File::open(&args.file)?;
  let mut contents = String::new();
  file.read_to_string(&mut contents)?;
  let data: Moldfile = toml::de::from_str(&contents)?;

  // optionally spew the parsed structure
  if args.debug {
    dbg!(&data);
  }

  // find our mold recipe dir and create it if it doesn't exist
  let mut mold_dir = args.file.clone();
  mold_dir.pop();
  mold_dir.push(&data.recipe_dir);
  let mold_dir = fs::canonicalize(mold_dir)?;

  if !mold_dir.is_dir() {
    fs::create_dir(&mold_dir)?;
  }

  if args.debug {
    dbg!(&mold_dir);
  }

  // clone or update all of our remotes if we haven't already
  // FIXME how should this work recursively...?
  for (name, recipe) in &data.recipes {
    match recipe {
      mold::Recipe::Script(_) => {}
      mold::Recipe::Group(g) => {
        let mut pb = mold_dir.clone();
        pb.push(name);
        if !pb.is_dir() {
          remote::clone(&g.url, &pb)?;
        } else if args.update {
          remote::checkout(&pb, &g.ref_)?;
        }
      }
    }
  }

  /*
  // FIXME this is a little broken for now because of the new remote things,
  // but I'm planning to make it so the groups act as a namespace, eg:
  //   $ mold this:shell
  // is the equivalent of:
  //   $ mold -f .mold/this/moldfile shell

  // FIXME also target can be None, in which case we should just list out the name and help values
  // for everything, and then maybe recurse into groups?

  // which recipe we're trying to execute
  let target = data
    .recipes
    .get(&args.target)
    .ok_or_else(|| failure::err_msg("couldn't locate target"))?;

  // what the interpreter is for this recipe
  let type_ = data
    .types
    .get(&target.type_)
    .ok_or_else(|| failure::err_msg("couldn't locate type"))?;

  // find the script file to execute
  let script = match &target.script {
    // either it was explicitly set in the moldfile, or...
    Some(x) => {
      let mut pb = mold_dir.clone()
      pb.push(x);
      pb
    }

    // we need to look it up based on our interpreter's known extensions
    None => type_.find(&mold_dir, &args.target)?,
  };

  type_.exec(&script)?;
  */

  Ok(())
}
