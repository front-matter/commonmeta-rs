use clap::Command;

pub mod crockford;
pub mod decode;
pub mod doi_utils;
pub mod encode;
pub mod utils;

fn main() -> Result<(), String> {
    let matches = Command::new("commonmeta")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Front Matter <info@front-matter.io>")
        .about("Commonmeta")
        .subcommand(encode::command())
        .subcommand(decode::command())
        .get_matches();

    match matches.subcommand() {
        Some(("encode", sub_matches)) => encode::execute(sub_matches),
        Some(("decode", sub_matches)) => decode::execute(sub_matches),
        _ => Ok(()),
    }
}
