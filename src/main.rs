use clap::Command;

mod cmd;
pub mod crockford;
pub mod doi_utils;
pub mod file_utils;
pub mod progress;
pub mod utils;

fn main() -> Result<(), String> {
    let matches = Command::new("commonmeta")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Front Matter <info@front-matter.de>")
        .about("Commonmeta")
        .subcommand(cmd::convert::command())
        .subcommand(cmd::encode::command())
        .subcommand(cmd::decode::command())
        .subcommand(cmd::list::command())
        .subcommand(cmd::push::command())
        .subcommand(cmd::put::command())
        .subcommand(cmd::r#match::command())
        .get_matches();

    match matches.subcommand() {
        Some(("convert", sub_matches)) => cmd::convert::execute(sub_matches),
        Some(("encode", sub_matches)) => cmd::encode::execute(sub_matches),
        Some(("decode", sub_matches)) => cmd::decode::execute(sub_matches),
        Some(("list", sub_matches)) => cmd::list::execute(sub_matches),
        Some(("push", sub_matches)) => cmd::push::execute(sub_matches),
        Some(("put", sub_matches)) => cmd::put::execute(sub_matches),
        Some(("match", sub_matches)) => cmd::r#match::execute(sub_matches),
        _ => Ok(()),
    }
}
