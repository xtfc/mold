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

  #[structopt(long = "foo", short = "o")]
  pub foo: bool,

  /// Which recipe to run
  pub target: String,
}

fn main() -> Result<(), ExitFailure> {
  let args = Root::from_args();
  env_logger::init();

  // read and deserialize the moldfile
  let mut file = File::open(args.file)?;
  let mut contents = String::new();
  file.read_to_string(&mut contents)?;
  let data: Moldfile = toml::de::from_str(&contents)?;

  // optionally spew the parsed structure
  if args.debug {
    dbg!(&data);
  }

  // create a .mold directory to store our groups
  let mold_dir = PathBuf::from(".mold");
  if !mold_dir.is_dir() {
    fs::create_dir(".mold")?;
  }

  // clone all of our remotes if we can
  for (name, recipe) in &data.recipes {
    match recipe {
      mold::Recipe::Script(_) => {}
      mold::Recipe::Group(g) => {
        let mut pb = mold_dir.clone();
        pb.push(name);
        if !pb.is_dir() {
          remote::clone(&g.url, &pb)?;
        }
      }
    }
  }

  /* FIXME this is a little broken for now.
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
      let mut pb = PathBuf::from(&data.recipe_dir);
      pb.push(x);
      pb
    }

    // we need to look it up based on our interpreter's known extensions
    None => type_.find(Path::new(&data.recipe_dir), &args.target)?,
  };

  type_.exec(&script)?;
  */

  Ok(())
}
