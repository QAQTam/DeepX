fn main() {
    // Re-run build script when frontend dist changes (so cargo recompiles the binary).
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build()
}