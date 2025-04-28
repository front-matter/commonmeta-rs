/*
 * Copyright © 2025 Front Matter <info@front-matter.io>
 */

use clap::{ArgMatches, Command};

use crate::utils::decode_id;

/// Build the decode subcommand
pub fn command() -> Command {
    Command::new("decode")
        .about("Decode an identifier.")
        .long_about(
            "Decode a DOI, ROR or ORCID identifier. For DOIs only Crockford \
            base32-encoding is supported, used by Rogue Scholar and some DataCite \
            members.\n\n\
            Example usage:\n\n\
            commonmeta decode 10.54900/d3ck1-skq19",
        )
        .arg(
            clap::Arg::new("identifier")
                .help("Identifier to decode")
                .required(true)
                .index(1),
        )
}

/// Execute the decode command
pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let input = matches.get_one::<String>("identifier").expect("required");

    match decode_id(input) {
        Ok(number) => {
            println!("{}", number);
            Ok(())
        }
        Err(e) => {
            println!("{}", e);
            Err(e)
        }
    }
}
