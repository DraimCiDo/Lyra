pub fn allowed(path:&str)->bool {
    !path.is_empty() && !path.contains(['\\',':']) && path.split('/').all(|part| !part.is_empty() && !part.starts_with('.') && !part.chars().any(char::is_control))
}
