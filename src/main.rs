use clap::Parser;

mod cli;

fn main() {
    let _ = cli::Cli::try_parse_from(std::env::args());
}
