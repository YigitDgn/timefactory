use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=resources/app.rc.in");
    println!("cargo:rerun-if-changed=resources/logo.ico");
    println!("cargo:rerun-if-changed=resources/app.manifest");

    // Fill the version from Cargo.toml into the resource script so the exe's
    // file properties (Explorer > Properties > Details) match the app.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let res_dir = manifest_dir.join("resources");
    let template = fs::read_to_string(res_dir.join("app.rc.in")).expect("read app.rc.in");
    let rc = template
        .replace("@RES_DIR@", &res_dir.to_string_lossy().replace('\\', "/"))
        .replace("@VERSION@", &env::var("CARGO_PKG_VERSION").unwrap())
        .replace("@VER_MAJOR@", &env::var("CARGO_PKG_VERSION_MAJOR").unwrap())
        .replace("@VER_MINOR@", &env::var("CARGO_PKG_VERSION_MINOR").unwrap())
        .replace("@VER_PATCH@", &env::var("CARGO_PKG_VERSION_PATCH").unwrap());

    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app.rc");
    fs::write(&out, rc).expect("write app.rc");
    embed_resource::compile(&out, embed_resource::NONE);
}
