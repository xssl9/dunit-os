use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let repo_root = manifest_dir.parent().unwrap();
    let assets_dir = repo_root.join("assets");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_path = out_dir.join("assets_manifest.rs");

    println!("cargo:rerun-if-changed={}", assets_dir.display());

    let mut dirs = Vec::new();
    let mut files = Vec::new();
    collect_assets(&assets_dir, &assets_dir, &mut dirs, &mut files)
        .expect("failed to scan assets directory");

    dirs.sort();
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut output = String::new();
    output.push_str("pub static ASSET_DIRS: &[&str] = &[\n");
    for dir in dirs {
        output.push_str("    \"");
        output.push_str(&dir);
        output.push_str("\",\n");
    }
    output.push_str("];\n\n");

    output.push_str("pub static ASSETS: &[AssetEntry] = &[\n");
    for (vfs_path, host_path) in files {
        output.push_str("    AssetEntry { path: \"");
        output.push_str(&vfs_path);
        output.push_str("\", data: include_bytes!(r#\"");
        output.push_str(&host_path.display().to_string().replace('\\', "/"));
        output.push_str("\"#) },\n");
    }
    output.push_str("];\n");

    fs::write(manifest_path, output).expect("failed to write assets manifest");
}

fn collect_assets(
    root: &Path,
    dir: &Path,
    dirs: &mut Vec<String>,
    files: &mut Vec<(String, PathBuf)>,
) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap();
        let relative_vfs = relative.to_string_lossy().replace('\\', "/");

        if path.is_dir() {
            dirs.push(format!("/assets/{relative_vfs}"));
            println!("cargo:rerun-if-changed={}", path.display());
            collect_assets(root, &path, dirs, files)?;
        } else if path.is_file() {
            files.push((format!("/assets/{relative_vfs}"), path.canonicalize()?));
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    io::stdout().flush().ok();
    Ok(())
}
