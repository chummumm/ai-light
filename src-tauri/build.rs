mod icon_gen;
fn main() {
    println!("cargo:rerun-if-changed=icon_gen.rs");
    icon_gen::generate().expect("generate application and tray icons");
    tauri_build::build();
}
