//! Stamps the build date that `termoil +version` and the About box show, and on Windows embeds
//! the icon and the version resource.

/// Build script entry point.
fn main() {
    println!("cargo:rerun-if-env-changed=TERMOIL_BUILD_DATE");
    println!("cargo:rerun-if-changed=../../installer/windows/termoil.rc");
    println!("cargo:rerun-if-changed=../../installer/windows/termoil.ico");
    let date = std::env::var("TERMOIL_BUILD_DATE").unwrap_or_else(|_| "development build".into());
    println!("cargo:rustc-env=TERMOIL_BUILD_DATE={date}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_resource();
    }
}

/// Compiles installer/windows/termoil.rc with the package version into the executable.
fn embed_windows_resource() {
    let v = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let comma = format!("{},0", v.split('-').next().unwrap_or("0.0.0").replace('.', ","));
    let macros = [
        format!("TERMOIL_VERSION_STR=\"{v}\""),
        format!("TERMOIL_VERSION_COMMA={comma}"),
        "TERMOIL_FILE_DESCRIPTION=\"Termoil command line\"".to_string(),
    ];
    if let Err(e) = embed_resource::compile("../../installer/windows/termoil.rc", &macros).manifest_optional() {
        println!("cargo:warning=the Windows resource was not embedded: {e}");
    }
}
