fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if os == "macos" {
        cc::Build::new()
            .file("native/macos.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("native_ui");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
    } else if os == "windows" {
        cc::Build::new()
            .file("native/windows.c")
            .flag_if_supported("/utf-8")
            .compile("native_ui");
        for lib in ["user32", "shell32", "gdi32"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
    println!("cargo:rerun-if-changed=native");
}
