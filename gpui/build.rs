fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() != "windows" {
        return;
    }
    let target = std::env::var("TARGET").unwrap();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let icon = std::path::Path::new("../assets/app-icon.ico")
        .canonicalize()
        .unwrap();
    let rc = out.join("icon.rc");
    std::fs::write(
        &rc,
        format!(
            "1 ICON \"{}\"\n",
            icon.display().to_string().replace('\\', "\\\\")
        ),
    )
    .unwrap();
    let compiler = cc::windows_registry::find_tool(&target, "rc.exe")
        .expect("Windows SDK resource compiler is required");
    let resource = out.join("icon.res");
    assert!(
        compiler
            .to_command()
            .args(["/nologo", "/fo"])
            .arg(&resource)
            .arg(&rc)
            .status()
            .unwrap()
            .success()
    );
    println!("cargo:rustc-link-arg={}", resource.display());
    println!("cargo:rerun-if-changed=../assets/app-icon.ico");
}
