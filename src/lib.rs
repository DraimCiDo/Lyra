pub mod doctor;

use clap::{Parser, Subcommand};
use std::io::{self, Write};

#[derive(Debug, Parser)]
#[command(
    name = "lyra",
    version,
    about = "Локальный AI-агент для кодинга: диагностика ПК (M1)",
    long_about = "Основа локального AI-агента. Работает без сети и API-ключа.\nГенерация кода и загрузка модели ещё не реализованы.",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Показать характеристики компьютера, на котором запущена Lyra
    Doctor {
        /// Вывести JSON (схема v1), пригодный для подбора локальной модели
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cli: Cli, output: &mut impl Write) -> io::Result<()> {
    match cli.command {
        Command::Doctor { json } => {
            let report = doctor::collect()?;
            doctor::write_report(&report, json, output)
        }
    }
}
