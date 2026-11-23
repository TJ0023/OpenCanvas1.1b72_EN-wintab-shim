fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}\\wintab32.def", dir);
    println!("cargo:rerun-if-changed=wintab32.def");
}
