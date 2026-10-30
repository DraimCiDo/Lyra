#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--worker")) {
        return lyra::desktop::worker_main();
    }
    let smoke = std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--smoke-test"));
    let result = if smoke {
        lyra::desktop::smoke_test()
    } else {
        lyra::desktop::launch()
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if smoke {
                return std::process::ExitCode::FAILURE;
            }
            rfd::MessageDialog::new()
                .set_title("Lyra — ошибка запуска")
                .set_description(error.to_string())
                .set_level(rfd::MessageLevel::Error)
                .show();
            std::process::ExitCode::FAILURE
        }
    }
}
