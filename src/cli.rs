use anyhow::{bail, Result};
use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Config {
    pub(crate) force: bool,
    pub(crate) debug: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParsedArgs {
    Config(Config),
    HelpRequested,
}

pub(crate) fn parse_args() -> Result<Config> {
    match parse_args_from(env::args().skip(1))? {
        ParsedArgs::Config(cfg) => Ok(cfg),
        ParsedArgs::HelpRequested => {
            print_usage();
            std::process::exit(0);
        }
    }
}

pub(crate) fn parse_args_from<I, S>(args: I) -> Result<ParsedArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut force = false;
    let mut debug = false;

    for arg in args {
        match arg.as_ref() {
            "-f" | "--force" => force = true,
            "-d" | "--debug" => debug = true,
            "-h" | "--help" => return Ok(ParsedArgs::HelpRequested),
            other => bail!("Unknown argument: {other}"),
        }
    }

    Ok(ParsedArgs::Config(Config { force, debug }))
}

fn print_usage() {
    println!("gfresh - refresh a git repository");
    println!("Usage: gfresh [-f|--force] [-d|--debug]");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults() {
        let parsed = parse_args_from([] as [&str; 0]).unwrap();
        assert_eq!(
            parsed,
            ParsedArgs::Config(Config {
                force: false,
                debug: false
            })
        );
    }

    #[test]
    fn parse_force_and_debug() {
        let parsed = parse_args_from(["--force", "-d"]).unwrap();
        assert_eq!(
            parsed,
            ParsedArgs::Config(Config {
                force: true,
                debug: true
            })
        );
    }

    #[test]
    fn parse_help() {
        let parsed = parse_args_from(["--help"]).unwrap();
        assert_eq!(parsed, ParsedArgs::HelpRequested);
    }

    #[test]
    fn parse_unknown_arg_errors() {
        let err = parse_args_from(["--wat"]).unwrap_err().to_string();
        assert!(err.contains("Unknown argument"));
    }
}
