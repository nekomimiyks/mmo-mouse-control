use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-env-changed=WINDRES");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // MinGW tools need an ASCII path. The Windows build uses a temporary target directory.
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::copy("assets/icon.ico", out.join("icon.ico")).expect("copy the application icon");
    fs::write(out.join("icon.rc"), "1 ICON \"icon.ico\"\n").expect("write the icon resource script");

    let (mut compiler, output) = if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let mut compiler = Command::new("rc.exe");
        compiler.args(["/nologo", "/fo", "app-icon.res", "icon.rc"]);
        (compiler, "app-icon.res")
    } else {
        let mut compiler = Command::new(env::var_os("WINDRES").unwrap_or_else(|| "windres".into()));
        compiler.args(["-i", "icon.rc", "-O", "coff", "-o", "app-icon.o"]);
        (compiler, "app-icon.o")
    };
    let status = compiler.current_dir(&out).status().expect("run the Windows resource compiler");
    assert!(status.success(), "could not compile the Windows application icon");
    println!("cargo:rustc-link-arg-bin=mmo-mouse-control={}", out.join(output).display());
}
