use clap::{ArgMatches, Command};

pub fn command() -> Command {
    Command::new("push")
        .about("Push metadata records to a target")
        .long_about(
            "Push metadata records to a target (e.g. Crossref, DataCite).\n\n\
            Not yet implemented.",
        )
}

pub fn execute(_matches: &ArgMatches) -> Result<(), String> {
    Err("push: not yet implemented".to_string())
}
