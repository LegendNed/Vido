use std::path::{Path, PathBuf};

fn main() {
    copy_ffmpeg_dlls();
    tauri_build::build();
}

/// FFmpeg is linked dynamically. Copy its DLLs next to the dev/test binaries
/// (`target/<profile>` and `target/<profile>/deps`) so `tauri dev` and
/// `cargo test` find them. Release bundles ship them via `bundle.resources`.
fn copy_ffmpeg_dlls() {
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    let Some(ffmpeg_dir) = std::env::var_os("FFMPEG_DIR").map(PathBuf::from) else {
        println!("cargo:warning=FFMPEG_DIR is not set; run scripts/fetch-ffmpeg.ps1");
        return;
    };
    let bin = ffmpeg_dir.join("bin");
    let Ok(entries) = std::fs::read_dir(&bin) else {
        println!(
            "cargo:warning=FFmpeg DLLs not found in {}; run scripts/fetch-ffmpeg.ps1",
            bin.display()
        );
        return;
    };
    // OUT_DIR = target/<profile>/build/<pkg>-<hash>/out
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let Some(profile_dir) = out.ancestors().nth(3) else {
        return;
    };
    let targets = [profile_dir.to_path_buf(), profile_dir.join("deps")];
    for entry in entries.flatten() {
        let src = entry.path();
        if src.extension().and_then(|e| e.to_str()) != Some("dll") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", src.display());
        for dir in &targets {
            let _ = std::fs::create_dir_all(dir);
            let dst = dir.join(entry.file_name());
            if needs_copy(&src, &dst) {
                if let Err(e) = std::fs::copy(&src, &dst) {
                    println!("cargo:warning=copying {} failed: {e}", src.display());
                }
            }
        }
    }
}

fn needs_copy(src: &Path, dst: &Path) -> bool {
    match (std::fs::metadata(src), std::fs::metadata(dst)) {
        (Ok(s), Ok(d)) => s.len() != d.len() || s.modified().ok() > d.modified().ok(),
        _ => true,
    }
}
