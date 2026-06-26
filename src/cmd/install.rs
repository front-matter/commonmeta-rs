/*
 * Copyright © 2026 Front Matter <info@front-matter.de>
 */

//! Backward-compatible `install` command — supports `ror` only.
//! New installs should use `commonmeta import`.

use clap::{Arg, ArgMatches, Command};

use crate::cmd::resolve_db_path;
use crate::cmd::import;

pub fn command() -> Command {
    Command::new("install")
        .about("Install a vocabulary as a local SQLite database (use 'import' for metadata)")
        .long_about(
            "Download and install a controlled vocabulary for offline use.\n\n\
            'ror' — fetches the latest ROR release from Zenodo and stores all \
            organizations in a local SQLite database with a full-text index. \
            Used by 'commonmeta match' and 'commonmeta convert'.\n\n\
            This command is kept for backwards compatibility. \
            Use 'commonmeta import' for importing scholarly metadata records.\n\n\
            Supported vocabularies: ror\n\n\
            Examples:\n\n\
            commonmeta install ror\n\
            commonmeta install ror --file /data/ror.sqlite3",
        )
        .arg(
            Arg::new("vocabulary")
                .help("Vocabulary to install (only 'ror' is supported)")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("file")
                .long("file")
                .value_name("FILE")
                .help(
                    "Output SQLite file path. Overrides COMMONMETA_DB and the \
                    platform default.",
                ),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let vocabulary = matches.get_one::<String>("vocabulary").expect("required");
    let out_path = resolve_db_path(matches.get_one::<String>("file"));

    match vocabulary.as_str() {
        "ror" => import::install_ror(&out_path),
        other => Err(format!(
            "unsupported vocabulary '{}'. Use 'commonmeta import --from {}' instead.",
            other, other
        )),
    }
}
