use std::process::Command;

/// Links the system libraries of the text renderer (`rust/src/text.rs`):
/// PangoFT2 and fontconfig, found with `pkg-config` like in the C++ Makefile.
fn link_pkg_config(packages: &[&str]) {
    let output = Command::new("pkg-config")
        .arg("--libs")
        .args(packages)
        .output()
        .expect("running pkg-config failed (pkg-config and the pangoft2 development files are required)");
    if !output.status.success() {
        panic!(
            "pkg-config --libs {} failed: {}",
            packages.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for flag in String::from_utf8(output.stdout).unwrap().split_whitespace() {
        if let Some(lib) = flag.strip_prefix("-l") {
            println!("cargo::rustc-link-lib={lib}");
        } else if let Some(dir) = flag.strip_prefix("-L") {
            println!("cargo::rustc-link-search=native={dir}");
        }
    }
}

fn main() {
    println!("cargo::rerun-if-changed=rust/build.rs");
    println!("cargo::rerun-if-env-changed=PKG_CONFIG_PATH");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        // Like the original C++ build: use DT_RPATH (not DT_RUNPATH) pointing to the
        // directory of the executable, so that both libcef.so and vice plugins given
        // by a bare file name (such as the default retrojsvice.so) are found next to
        // the binary when loaded with dlopen.
        println!("cargo::rustc-link-arg-bins=-Wl,--disable-new-dtags");
        println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");

        link_pkg_config(&["pangoft2", "fontconfig"]);

        // XSetErrorHandler/XSetIOErrorHandler (rust/src/x_error.rs), -lX11 as in
        // the C++ Makefile.
        println!("cargo::rustc-link-lib=X11");
    }
}
