use clap::{Arg, ArgMatches, Command};

pub fn command() -> Command {
    Command::new("match")
        .about("Match a string to an identifier")
        .long_about(
            "Match a string to an identifier. Supports affiliation matching for ROR.\n\n\
            Example usage:\n\n\
            commonmeta match \"Leibniz Universität Hannover\"",
        )
        .arg(
            Arg::new("input")
                .help("The string to match, e.g. an affiliation name")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("from")
                .long("from")
                .short('f')
                .help("Input format to match against (currently only 'ror' is supported)")
                .default_value("ror"),
        )
        .arg(
            Arg::new("to")
                .long("to")
                .short('t')
                .help("Output format: ror (raw ROR JSON) or inveniordm (vocabulary YAML)")
                .default_value("ror"),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let input = matches.get_one::<String>("input").expect("required");
    let from = matches
        .get_one::<String>("from")
        .map(String::as_str)
        .unwrap_or("ror");
    let to = matches
        .get_one::<String>("to")
        .map(String::as_str)
        .unwrap_or("ror");
    let to = if to.is_empty() || to == "commonmeta" {
        "ror"
    } else {
        to
    };

    if from != "ror" {
        println!("No valid input format. Currently only 'ror' is supported.");
        return Ok(());
    }

    let candidates = commonmeta::match_ror_affiliation(input).map_err(|e| e.to_string())?;
    let chosen = candidates.into_iter().find(|m| m.chosen);

    let organization = match chosen {
        Some(m) => m.organization,
        None => {
            println!("No match found");
            return Ok(());
        }
    };

    let output = match to {
        "inveniordm" => commonmeta::write("ror", &organization).map_err(|e| e.to_string())?,
        "ror" => commonmeta::write_ror_json(&organization).map_err(|e| e.to_string())?,
        other => return Err(format!("match: unsupported --to format: {}", other)),
    };

    println!("{}", String::from_utf8_lossy(&output));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_defaults() {
        let matches = command().get_matches_from(vec!["match", "Leibniz Universität Hannover"]);
        assert_eq!(
            matches.get_one::<String>("from").map(String::as_str),
            Some("ror")
        );
        assert_eq!(
            matches.get_one::<String>("to").map(String::as_str),
            Some("ror")
        );
        assert_eq!(
            matches.get_one::<String>("input").map(String::as_str),
            Some("Leibniz Universität Hannover")
        );
    }

    #[test]
    fn test_unsupported_from_returns_message_not_error() {
        let matches =
            command().get_matches_from(vec!["match", "some affiliation", "--from", "grid"]);
        // Should print a message and return Ok, matching Go's behavior of
        // printing "No valid input format..." rather than erroring.
        let result = execute(&matches);
        assert!(result.is_ok());
    }
}
