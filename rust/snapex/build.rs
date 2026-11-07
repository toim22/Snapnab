//! The section with the hook's messages is shared by every process the DLL is mapped into (#pragma data_seg in C++).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-cdylib-link-arg=/SECTION:.shared,RWS");
    }
}
