fn main() {
  let manifest = match std::env::var("CARGO_MANIFEST_DIR") {
    Ok(m) => m,
    Err(_) => { tauri_build::build(); return; }
  };
  let profile = std::env::var("PROFILE").unwrap_or_default();
  if profile != "release" {
    tauri_build::build();
    return;
  }
  let m = std::path::Path::new(&manifest);
  let ws_root = match m.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
    Some(r) => r,
    None => { tauri_build::build(); return; }
  };
  let exe_name = format!("dsx{}", std::env::consts::EXE_SUFFIX);
  let src = ws_root.join("target").join(&profile).join(&exe_name);
  if src.exists() {
    if let Some(dest_dir) = m.join("resources").to_str().map(|_| m.join("resources")) {
      let _ = std::fs::create_dir_all(&dest_dir);
      let _ = std::fs::copy(&src, &dest_dir.join(&exe_name));
    }
  }
  tauri_build::build()
}
