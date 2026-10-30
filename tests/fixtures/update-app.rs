//! Native updater test application. Never loads a model or opens a window.
use std::{env, fs, thread, time::Duration};
fn main() {
    let args: Vec<_> = env::args_os().collect();
    let version = env!("LYRA_UPDATE_FIXTURE_VERSION");
    if args.get(1).is_some_and(|a| a == "--worker" || a == "--version") {
        println!("lyra {version}");
    } else if args.get(1).is_some_and(|a| a == "--hold") {
        loop { thread::sleep(Duration::from_secs(1)); }
    } else {
        let root = env::current_exe().unwrap().canonicalize().unwrap().parent().unwrap().to_path_buf();
        fs::write(root.join("started.txt"), version).unwrap();
        if args.get(1).is_some_and(|a| a == "--updated") && !root.join("fail-health").exists() {
            // The plan already has JSON-escaped canonical paths, including Windows extended paths.
            let dir = std::path::Path::new(&args[2]);
            let plan = fs::read_to_string(dir.join("plan.json")).unwrap();
            let prefix = "\"root\":";
            let start = plan.find(prefix).unwrap() + prefix.len();
            let end = plan[start..].find(",\"stage\"").unwrap() + start;
            fs::write(dir.join("healthy.json"), format!("{{\"version\":\"{version}\",\"root\":{}}}", &plan[start..end])).unwrap();
        }
    }
}
