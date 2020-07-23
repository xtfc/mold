use colored::*;
use exitfailure::ExitFailure;
use failure::Error;
use mold::Mold;
use std::path::Path;
use std::path::PathBuf;
use structopt::StructOpt;

// there's no good way that I could find to group these into exclusive groups.
// ie: `--clean` excludes everything else, `--import` permits `--prefix`, etc.
/// A fresh task runner
#[derive(StructOpt, Debug)]
#[structopt(author, global_settings(&[structopt::clap::AppSettings::ColoredHelp]))]
pub struct Args {
    /// Path to the moldfile
    #[structopt(long = "file", short = "f")]
    pub file: Option<PathBuf>,

    /// Comma-separated list of mold environments to activate
    #[structopt(long = "env", short = "e", env = "MOLDENV")]
    pub env: Option<String>,

    /// Single mold environment to append to list of active environments
    #[structopt(long = "add", short = "a", number_of_values = 1)]
    pub add_envs: Vec<String>,

    /// Add an import to the selected moldfile
    #[structopt(long = "import", short = "i")]
    pub import: Option<String>,

    /// Optional prefix to use with --import / -i
    #[structopt(long = "prefix", short = "p")]
    pub prefix: Option<String>,

    /// Fetch new updates for all downloaded remote data
    #[structopt(long = "update", short = "u")]
    pub update: bool,

    /// Remove all downloaded remote data
    #[structopt(long = "clean")]
    pub clean: bool,

    /// Download all remote data
    #[structopt(long = "clone")]
    pub clone: bool,

    /// Output a shell source-able listing of variables
    #[structopt(long = "vars")]
    pub vars: bool,

    /// Explain commands to be run rather than executing them
    #[structopt(long = "explain", short = "x")]
    pub explain: bool,

    /// Which recipe(s) to run
    pub targets: Vec<String>,
}

/// Handle actual execution
fn run(args: Args) -> Result<(), Error> {
    // load the moldfile
    let mut envs = vec![];
    envs.extend(args.env);
    envs.extend(args.add_envs);
    envs.push(std::env::consts::FAMILY.to_string());
    envs.push(std::env::consts::OS.to_string());

    let filepath = Mold::discover(&Path::new("."), args.file.clone())?;

    // early return if we passed a --clean
    if args.clean {
        return Mold::clean_all(&filepath);
    }

    if let Some(import) = args.import {
        use std::io::prelude::*;
        let line = if let Some(prefix) = args.prefix {
            format!("import \"{}\" as {}\n", import, prefix)
        } else {
            format!("import \"{}\"\n", import)
        };

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&filepath)
            .map_err(|err| {
                failure::format_err!(
                    "Couldn't open file {} for appending: {}",
                    filepath.display().to_string().red(),
                    err
                )
            })?;
        file.write_all(line.as_bytes())?;
        return Ok(());
    }

    let mold = Mold::init(&filepath, envs)?;

    // early return if we passed a --update
    if args.update {
        return mold.update_all();
    }

    // list all variables if they're set
    if args.vars {
        mold.sh_vars()?;
        return Ok(());
    }

    // early return and print help if we didn't pass any targets
    if args.targets.is_empty() {
        return mold.help();
    }

    // explain all of the given targets rather than executing them
    if args.explain {
        for target_name in &args.targets {
            mold.explain(target_name)?;
        }

        return Ok(());
    }

    let requested_targets = args
        .targets
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    let all_targets = mold.find_all_dependencies(&requested_targets)?;

    for target_name in &all_targets {
        mold.execute(target_name)?;
    }

    Ok(())
}

/// Facade to work with ExitFailure
fn main() -> Result<(), ExitFailure> {
    let args = Args::from_args();
    env_logger::init();

    run(args)?;

    Ok(())
}
