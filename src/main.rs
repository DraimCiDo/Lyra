use clap::Parser;
use lyra::{Cli, run};
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code() as u8;
            if let Err(output_error) = error.print() {
                return output_failure(output_error);
            }
            return ExitCode::from(code);
        }
    };
    match run(cli, &mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => output_failure(error),
    }
}

fn output_failure(error: io::Error) -> ExitCode {
    if error.kind() == io::ErrorKind::BrokenPipe {
        return ExitCode::SUCCESS;
    }
    let _ = writeln!(io::stderr().lock(), "Ошибка: {error}");
    match error.kind() {
        io::ErrorKind::Interrupted => ExitCode::from(130),
        io::ErrorKind::TimedOut => ExitCode::from(124),
        _ => ExitCode::FAILURE,
    }
}
