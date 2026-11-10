fn main() {
    slint_build::compile("ui/main.slint").expect("Slint build failed");

    // Windows: embed the app icon and the name shown in Task Manager / file properties.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/kiln.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/kiln.ico")
            .set("ProductName", "Kiln")
            .set("FileDescription", "Kiln")
            .set("OriginalFilename", "Kiln.exe");
        if let Err(e) = res.compile() {
            // Not fatal: the app still runs, just with the default icon.
            println!("cargo:warning=could not embed Windows icon: {e}");
        }
    }
}
