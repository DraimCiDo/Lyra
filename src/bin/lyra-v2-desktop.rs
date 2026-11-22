#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result {
    lyra::v2::desktop::launch(std::env::args().any(|argument| argument == "--smoke-test"))
}
