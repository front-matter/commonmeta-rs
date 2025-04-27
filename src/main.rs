mod crockford;
mod decode;
mod doiutils;
mod encode;
mod utils;

use clap::Command;

fn main() -> Result<(), String> {
    let matches = Command::new("commonmeta")
        .version("0.1.0")
        .author("Front Matter <info@front-matter.io>")
        .about("CommonMeta tools")
        .subcommand(encode::command())
        .subcommand(decode::command())
        .get_matches();

    match matches.subcommand() {
        Some(("encode", sub_matches)) => encode::execute(sub_matches),
        Some(("decode", sub_matches)) => decode::execute(sub_matches),
        _ => Ok(()),
    }
}
