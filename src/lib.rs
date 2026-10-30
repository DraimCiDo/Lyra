pub mod agent;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod doctor;
pub mod inference;
pub mod workspace;

use clap::{Parser, Subcommand};
use std::io::{self, Write};

#[derive(Debug, Parser)]
#[command(
    name = "lyra",
    version,
    about = "Локальная генерация текста и диагностика ПК",
    long_about = "Локальный кодинг-агент: task, предложения правок и явный apply.\nПроизвольные команды модели ещё не реализованы.",
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Выполнить задачу: чтение проекта, предложения, опциональные подагенты
    Task(agent::TaskArgs),
    /// Применить одно выбранное предложение из завершённого JSON-отчёта
    Apply(agent::ApplyArgs),
    /// Сгенерировать ответ локальной моделью (не исполняет сгенерированный код)
    Generate(inference::GenerateArgs),
    /// Показать характеристики компьютера, на котором запущена Lyra
    Doctor {
        /// Вывести JSON (схема v1), пригодный для подбора локальной модели
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cli: Cli, output: &mut impl Write) -> io::Result<()> {
    match cli.command {
        Command::Task(args) => agent::task(args, output),
        Command::Apply(args) => agent::apply(args, output),
        Command::Generate(args) => inference::generate(&args, output),
        Command::Doctor { json } => {
            let report = doctor::collect()?;
            doctor::write_report(&report, json, output)
        }
    }
}
