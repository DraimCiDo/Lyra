#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let smoke = arguments.iter().any(|argument| argument == "--smoke-test");
    // `--hub <folder>` opens the Agents view on an existing agent hub.
    let hub = arguments
        .iter()
        .position(|argument| argument == "--hub")
        .and_then(|index| arguments.get(index + 1))
        .map(std::path::PathBuf::from);
    lyra::v2::desktop::launch(smoke, hub)
}
