/*
 * Copyright © 2025 Front Matter <info@front-matter.io>
 */

use clap::{ArgMatches, Command};

use crate::doi_utils::encode_doi;
use crate::doi_utils::validate_prefix;

/// Build the encode subcommand
pub fn command() -> Command {
    Command::new("encode")
        .about("Generate a random DOI string given a prefix")
        .long_about(
            "Generate a random DOI string given a prefix. Example usage:\n\n\
            commonmeta encode 10.5555",
        )
        .arg(
            clap::Arg::new("prefix")
                .help("DOI prefix")
                .required(true)
                .index(1),
        )
}

/// Execute the encode command
pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let input = matches.get_one::<String>("prefix").expect("required");

    let prefix = match validate_prefix(input) {
        Some(p) => p,
        None => return Err("Invalid prefix".to_string()),
    };

    let doi = encode_doi(&prefix);
    println!("{}", doi);

    Ok(())
}
