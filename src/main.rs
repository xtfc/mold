use colored::*;
use exitfailure::ExitFailure;
use failure::Error;
use mold::remote;
use mold::Moldfile;
use mold::Recipe;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use structopt::StructOpt;

/// A fresh task runner
#[derive(StructOpt, Debug)]
#[structopt(raw(setting = "structopt::clap::AppSettings::ColoredHelp"))]
pub struct Args {
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
  let args = Args::from_args();
  env_logger::init();

  run(args)?;

  Ok(())
}

fn run(args: Args) -> Result<(), Error> {
  // read and deserialize the moldfile
  // FIXME this should probably do a "discover"-esque thing and crawl up the tree
  // looking for one
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
  for (name, recipe) in &data.recipes {
    match recipe {
      Recipe::Script(_) => {}
      Recipe::Group(group) => {
        let mut pb = mold_dir.clone();
        pb.push(name);

        if !pb.is_dir() {
          remote::clone(&group.url, &pb)?;
          remote::checkout(&pb, &group.ref_)?;
        } else if args.update {
          remote::checkout(&pb, &group.ref_)?;
        }
      }
    }
  }

  if args.update {
    return Ok(());
  }

  match &args.target {
    None => {
      for (name, recipe) in &data.recipes {
        let help = match recipe {
          Recipe::Script(s) => &s.help,
          Recipe::Group(g) => &g.help,
        };
        println!("{:>12} {}", name.cyan(), help);
      }
    }
    Some(target_name) => {
      if target_name.contains('/') {
        let splits: Vec<_> = target_name.splitn(2, '/').collect();
        let group_name = splits[0];
        let recipe_name = splits[1];

        let target = data
          .recipes
          .get(group_name)
          .ok_or_else(|| failure::err_msg("couldn't locate target group"))?;

        let target = match target {
          Recipe::Script(_) => {
            return Err(failure::err_msg("Can't execute a subrecipe of a script"))
          }
          Recipe::Group(target) => target,
        };

        let new_args = Args {
          file: mold_dir.join(group_name).join(&target.file),
          target: Some(recipe_name.to_string()),
          ..args
        };
        run(new_args)?;
      } else {
        let target = data
          .recipes
          .get(target_name)
          .ok_or_else(|| failure::err_msg("couldn't locate target"))?;

        // unwrap the script or quit
        let target = match target {
          Recipe::Script(target) => target,
          Recipe::Group(_) => return Err(failure::err_msg("Can't execute a group")),
        };

        // what the interpreter is for this recipe
        let type_ = data
          .types
          .get(&target.type_)
          .ok_or_else(|| failure::err_msg("couldn't locate type"))?;

        // find the script file to execute
        let script = match &target.script {
          // either it was explicitly set in the moldfile, or...
          Some(x) => {
            let mut pb = mold_dir.clone();
            pb.push(x);
            pb
          }

          // we need to look it up based on our interpreter's known extensions
          None => type_.find(&mold_dir, &target_name)?,
        };

        type_.exec(&script)?;
      }
    }
  }

  Ok(())
}
