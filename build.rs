fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    #[cfg(all(windows, feature = "desktop"))]
    {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/app.ico")
            .set("ProductName", "Lyra")
            .set("FileDescription", "Lyra local coding agent");
        if let Err(error) = resource.compile_for("lyra-desktop") {
            panic!("Could not embed Lyra icon: {error}");
        }
    }
}
