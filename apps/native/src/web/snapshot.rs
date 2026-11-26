//! A web view's snapshots: saved as PNGs in its plugin's web folder, no more than `LONGEST` pixels on their long side, so
//! one goes to Claude as an image it reads whole. A plugin's tool hands one on by its path (`attach`), which has to be
//! one of these files: a plugin can't have Wings read any other.

use std::path::{Path, PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

/// The most pixels on a snapshot's long side.
pub const LONGEST: u32 = 1600;
/// The snapshots kept in a folder: the oldest go past this.
const KEPT: usize = 20;

/// Saves `png` in `dir`, scaled down to `LONGEST` if it is bigger: `{ path, width, height }`.
pub fn save(dir: &Path, png: &[u8]) -> Result<Value, String> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).map_err(|error| error.to_string())?;
    let (width, height) = (image.width(), image.height());
    let image = if width.max(height) > LONGEST {
        image.resize(LONGEST, LONGEST, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
    // Two in the same millisecond (two pages at once) each keep their own.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("{now}x{next}.png"));
    image.save_with_format(&path, image::ImageFormat::Png).map_err(|error| error.to_string())?;
    prune(dir);
    Ok(json!({ "path": path, "width": image.width(), "height": image.height() }))
}

/// Keeps the newest `KEPT` snapshots in `dir`.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut shots: Vec<PathBuf> = entries.filter_map(|entry| Some(entry.ok()?.path())).filter(|path| path.extension().is_some_and(|ext| ext == "png")).collect();
    // Named by the time they were taken, so their names sort oldest first.
    shots.sort();
    let extra = shots.len().saturating_sub(KEPT);
    for old in &shots[..extra] {
        let _ = std::fs::remove_file(old);
    }
}

/// The image content of the snapshot at `path` for a tool's answer, when it is one of the plugin's (`web_dir`'s).
pub fn attach(web_dir: &Path, path: &str) -> Result<Value, String> {
    let shots = web_dir.join("screenshots");
    let path = Path::new(path);
    let inside = path.parent().is_some_and(|parent| parent == shots)
        && path.extension().is_some_and(|ext| ext == "png")
        && path.file_name().is_some_and(|name| name.to_string_lossy().bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'.'));
    if !inside {
        return Err("A tool's image is one of its plugin's web snapshots".into());
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    Ok(json!({ "type": "image", "mimeType": "image/png", "data": STANDARD.encode(bytes) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::new(width, height).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        bytes
    }

    #[test]
    fn a_snapshot_is_saved_no_bigger_than_claude_reads_and_only_those_attach() {
        let dir = tempfile::tempdir().unwrap();
        let web = dir.path().join("plugin-web/browser.s");
        let shots = web.join("screenshots");
        let saved = save(&shots, &png(3200, 1000)).unwrap();
        assert_eq!((saved["width"].as_u64(), saved["height"].as_u64()), (Some(1600), Some(500)), "the long side at 1600");
        let small = save(&shots, &png(390, 844)).unwrap();
        assert_ne!(small["path"], saved["path"], "each its own file");
        assert_eq!((small["width"].as_u64(), small["height"].as_u64()), (Some(390), Some(844)), "never scaled up");
        let path = saved["path"].as_str().unwrap();
        assert_eq!(attach(&web, path).unwrap()["mimeType"], "image/png");
        // Any other file, even one beside them, isn't read.
        std::fs::write(web.join("secret.png"), b"x").unwrap();
        assert!(attach(&web, &web.join("secret.png").to_string_lossy()).is_err());
        assert!(attach(&web, &format!("{}/../secret.png", shots.display())).is_err());
        assert!(attach(&web, "/etc/passwd").is_err());
    }
}
