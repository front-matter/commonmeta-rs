use clap::{ArgMatches, Command};

pub fn command() -> Command {
    Command::new("list")
        .about("List metadata records")
        .long_about(
            "List metadata records from a source.\n\n\
            Not yet implemented.",
        )
}

pub fn execute(_matches: &ArgMatches) -> Result<(), String> {
    Err("list: not yet implemented".to_string())
}
