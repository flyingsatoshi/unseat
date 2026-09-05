fn main() {
    println!("cargo:rerun-if-changed=assets/unseat.ico");
    println!("cargo:rerun-if-changed=unseat.rc");
    if std::env::var_os("CARGO_FEATURE_GUI").is_none() {
        return;
    }
    embed_resource::compile_for("unseat.rc", &["unseat"], embed_resource::NONE)
        .manifest_optional()
        .expect("embed Unseat icon");
}
