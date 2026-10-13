fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if os == "macos" {
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let png = std::fs::read("assets/icon-128.png").expect("Missing application icon");
        let header = format!(
            "static const unsigned char WB_ICON_PNG[] = {{{}}};\n",
            png.iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        std::fs::write(out.join("icon_png.h"), header).unwrap();
        cc::Build::new()
            .include(&out)
            .file("native/macos.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .compile("native_ui");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
    } else if os == "windows" {
        let target = std::env::var("TARGET").unwrap();
        let rc = cc::windows_registry::find_tool(&target, "rc.exe")
            .expect("Windows SDK resource compiler rc.exe is required");
        let res =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("app-icon.res");
        let status = rc
            .to_command()
            .args(["/nologo", "/fo"])
            .arg(&res)
            .arg("native/app-icon.rc")
            .status()
            .expect("Cannot run Windows resource compiler");
        assert!(status.success(), "Cannot compile application icon resource");
        println!("cargo:rustc-link-arg={}", res.display());
        cc::Build::new()
            .file("native/windows.c")
            .flag_if_supported("/utf-8")
            .compile("native_ui");
        for lib in ["user32", "shell32", "gdi32"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
    println!("cargo:rerun-if-changed=native");
    println!("cargo:rerun-if-changed=assets");
}
