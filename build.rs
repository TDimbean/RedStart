fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winres::WindowsResource::new()
            .set_icon("assets/redstart.ico")
            .set("ProductName", "RedStart")
            .compile()
            .expect("Windows icon resource compilation failed");
    }
}
