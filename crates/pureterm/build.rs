fn main() {
    // PureTerm v0.2: 在每个平台上为二进制打上 `VERSION` 与 `BUILD_TARGET` 环境变量,
    // 使 `--version` / 关于框可直接显示,
    // 无需从 `CARGO_PKG_VERSION` 重新推导(那会漏掉
    // build.sh 在打包时注入的 commit 后缀)。
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

    // PureTerm v0.2: Windows 资源嵌入在 v1 是空操作(我们发布
    // 便携 .zip 而非 .msi),但仍记录 rerun-if-changed
    // 钩子:日后放入 `windows/pureterm.rc` 即可触发重编译,
    // 无需 `cargo clean`。
    #[cfg(windows)]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let rc = std::path::Path::new(&manifest_dir)
            .join("windows")
            .join("pureterm.rc");
        if rc.exists() {
            println!("cargo:rerun-if-changed={}", rc.display());
            // 实际的 `embed_resource::compile(...)` 调用在 v1 刻意
            // 未添加——`embed_resource` 尚不是构建依赖。
            // 真正的 .rc 加入时,也要把 `embed_resource` 加进
            // `[target.'cfg(windows)'.build-dependencies]`.
        }
    }
}
