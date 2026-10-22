fn main() {
    // The Windows resource (exe / taskbar / installer icon) is only regenerated when the build
    // script reruns; make icon changes trigger that.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
