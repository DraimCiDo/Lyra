use clap::Parser;
use lyra::v2::cli;
use std::{io, process::ExitCode};

fn main() -> ExitCode {
    match cli::run(cli::Cli::parse(), &mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(match error.kind() {
                io::ErrorKind::Interrupted => 130,
                io::ErrorKind::TimedOut => 124,
                _ => 1,
            })
        }
    }
}
