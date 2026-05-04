use clap::Parser;

fn main() {
    let _ = switchbot::cli::Cli::try_parse_from(std::env::args());
}
