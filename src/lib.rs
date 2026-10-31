pub mod agent;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod doctor;
pub mod inference;
pub mod knowledge;
pub mod model_gateway;
pub mod project;
pub mod skills;
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
    /// Создание проектов с нуля: blueprint, каркас, явное создание, контекст
    Project(project::ProjectArgs),
    /// Локальная документация, поиск и явная загрузка официальных источников
    Docs(knowledge::DocsArgs),
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
        Command::Project(args) => project::run(args, output),
        Command::Docs(args) => knowledge::run(args, output),
        Command::Task(args) => agent::task(args, output),
        Command::Apply(args) => agent::apply(args, output),
        Command::Generate(args) => inference::generate(&args, output),
        Command::Doctor { json } => {
            let report = doctor::collect()?;
            doctor::write_report(&report, json, output)
        }
    }
}
