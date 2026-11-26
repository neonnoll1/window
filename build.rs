use std::{env, path::PathBuf, process::Command};

fn main() {
    for path in [
        "assets/windru.rc",
        "assets/windru.ico",
        "assets/windru.manifest",
        "assets/third-party-notices.txt",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let target = env::var("TARGET").unwrap();
    if !target.contains("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let resource = if target.ends_with("msvc") {
        let resource = out.join("windru.res");
        let status = Command::new("rc.exe")
            .arg("/nologo")
            .arg(format!("/fo{}", resource.display()))
            .arg("assets/windru.rc")
            .status()
            .expect("Для сборки MSVC требуется Windows SDK (rc.exe)");
        assert!(status.success(), "Не удалось собрать ресурс Windows");
        resource
    } else {
        let resource = out.join("windru.o");
        let windres = env::var("WINDRES").unwrap_or_else(|_| {
            if env::var("HOST").unwrap().contains("windows") {
                "windres".into()
            } else {
                "x86_64-w64-mingw32-windres".into()
            }
        });
        let status = Command::new(windres)
            .args(["-i", "assets/windru.rc", "-O", "coff", "-o"])
            .arg(&resource)
            .status()
            .expect("Для сборки GNU требуется windres");
        assert!(status.success(), "Не удалось собрать ресурс Windows");
        resource
    };
    println!("cargo:rustc-link-arg-bin=WindRu={}", resource.display());
}
