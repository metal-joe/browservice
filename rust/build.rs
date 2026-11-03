fn main() {
    println!("cargo::rerun-if-changed=rust/build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        // Like the original C++ build: use DT_RPATH (not DT_RUNPATH) pointing to the
        // directory of the executable, so that both libcef.so and vice plugins given
        // by a bare file name (such as the default retrojsvice.so) are found next to
        // the binary when loaded with dlopen.
        println!("cargo::rustc-link-arg-bins=-Wl,--disable-new-dtags");
        println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
}
