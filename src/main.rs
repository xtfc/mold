// use failure::ResultExt;
use exitfailure::ExitFailure;
use mold::Moldfile;
use std::fs::File;
use std::io::prelude::*;
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

  /// Which recipe to run
  pub recipe: String,
}

fn main() -> Result<(), ExitFailure> {
  let args = Root::from_args();
  env_logger::init();

  let mut file = File::open(args.file)?;
  let mut contents = String::new();
  file.read_to_string(&mut contents)?;

  let data: Moldfile = toml::de::from_str(&contents)?;

  dbg!(data);

  Ok(())
}
