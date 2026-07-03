fn main() {
    // PureTerm v0.2: stamp `VERSION` and `BUILD_TARGET` env vars on every
    // platform so the binary's `--version` / about box can show them
    // without re-deriving from `CARGO_PKG_VERSION` (which misses the
    // commit suffix that build.sh injects at packaging time).
    let mut version = String::from(env!("CARGO_PKG_VERSION"));
    if let Ok(v) = std::env::var("PURETERM_VERSION_OVERRIDE") {
        if !v.is_empty() {
            version = v;
        }
    }
    println!("cargo:rustc-env=VERSION={version}");
    if let Ok(t) = std::env::var("PURETERM_BUILD_TARGET") {
        println!("cargo:rustc-env=BUILD_TARGET={t}");
    }

    #[cfg(target_os = "macos")]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let plist = std::path::Path::new(&manifest_dir).join("Info.plist");
        if plist.exists() {
            println!("cargo:rustc-link-arg=-sectcreate");
            println!("cargo:rustc-link-arg=__TEXT");
            println!("cargo:rustc-link-arg=__info_plist");
            println!("cargo:rustc-link-arg={}", plist.display());
        }
        println!("cargo:rerun-if-changed=Info.plist");
    }

    // PureTerm v0.2: Windows resource embedding is a no-op for v1 (we ship
    // a portable .zip rather than an .msi), but record a rerun-if-changed
    // hook so dropping a `windows/pureterm.rc` later will trigger a rebuild
    // without a `cargo clean`.
    #[cfg(windows)]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let rc = std::path::Path::new(&manifest_dir)
            .join("windows")
            .join("pureterm.rc");
        if rc.exists() {
            println!("cargo:rerun-if-changed={}", rc.display());
            // Actual `embed_resource::compile(...)` call is intentionally
            // not added here in v1 — `embed_resource` is not yet a build
            // dep. When a real .rc is added, also add `embed_resource` to
            // `[target.'cfg(windows)'.build-dependencies]`.
        }
    }
}
