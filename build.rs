fn main() {
    println!("cargo:rerun-if-changed=assets/app_icon.ico");

    #[cfg(target_os = "windows")]
    winres::WindowsResource::new()
        .set_icon("assets/app_icon.ico")
        .set("ProductName", "CRONI")
        .set("FileDescription", "CRONI - Navegador web ligero")
        .set("OriginalFilename", "CRONI.exe")
        .compile()
        .expect("no se pudieron incrustar los recursos de CRONI");
}
