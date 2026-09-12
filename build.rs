fn main() {
    println!("cargo:rerun-if-changed=assets/unseat.ico");
    println!("cargo:rerun-if-changed=unseat.rc");
    println!("cargo:rerun-if-changed=unseat.manifest");
    if std::env::var_os("CARGO_FEATURE_GUI").is_none() {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").expect("package version");
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR").unwrap();
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR").unwrap();
    let patch = std::env::var("CARGO_PKG_VERSION_PATCH").unwrap();
    let macros = [
        format!("UNSEAT_VERSION=\"{version}\""),
        format!("UNSEAT_VERSION_NUM={major},{minor},{patch},0"),
    ];
    let resources = embed_resource::compile_for("unseat.rc", ["unseat"], &macros);
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        resources
            .manifest_required()
            .expect("embed Unseat identity and application manifest");
    } else {
        resources
            .manifest_optional()
            .expect("compile optional development resources");
    }
}
