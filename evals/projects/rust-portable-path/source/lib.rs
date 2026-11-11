pub fn allowed(path:&str)->bool { !path.starts_with('/') && !path.contains("../") }
