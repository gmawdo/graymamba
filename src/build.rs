// build.rs
fn main() {
    println!("cargo:rustc-link-lib=static=secretsharing");
    println!("cargo:rustc-link-search=native=/Users/arifahmad/Rough_Block/secretsharing/target/release/libsecretsharing.a");
}