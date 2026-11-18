//! The pictures plugin pages show. A page registers a picture's bytes once (`wings.registerImage`, base64 of a png, jpeg,
//! gif or webp) under an id of its own and then names that id in an `Image` node, so the bytes never travel with the
//! tree. The app keeps what was registered within a budget, newest used first, and decodes a picture only when a page
//! draws it, once, on a thread of its own and at the size the page draws it (a big one is scaled down), so a page that
//! renders again as text streams in costs a lookup.

use std::{collections::HashMap, io::Cursor, sync::{Arc, Mutex, OnceLock}};

use base64::{Engine, engine::general_purpose::STANDARD};
use image::ImageFormat;
use sha2::{Digest, Sha256};

use super::tree::valid_image_id as valid_id;

/// The most bytes of one picture, which is what the Claude API takes of an image.
pub const MAX_BYTES: usize = 5 * 1024 * 1024;
/// The most pixels one may have: it is decoded in full before it is scaled down.
const MAX_PIXELS: u64 = 16_000_000;
/// What is kept: pictures and their bytes. The least recently shown go first, and a page then shows a note for them.
const MAX_ENTRIES: usize = 256;
const KEPT_BYTES: usize = 64 * 1024 * 1024;
/// The most ids pages have registered, across plugins.
const MAX_NAMES: usize = 4096;
/// The most bytes of decoded pixels kept, and the longest side a picture is decoded to.
const KEPT_DECODED: usize = 96 * 1024 * 1024;
pub const MAX_SIDE: u32 = 1600;

type Hash = [u8; 32];
/// Called once when a picture a page waited for is decoded, or is gone.
pub type Wake = Box<dyn FnOnce() + Send>;

/// A picture decoded to pixels the window can draw.
pub struct Decoded {
    pub image: Arc<gpui::RenderImage>,
    /// The longest side it was decoded to, or `u32::MAX` when that is the picture's own size.
    level: u32,
    bytes: usize,
}

enum Work {
    Idle,
    /// A decode at this longest side is queued or running, and these want to hear when it ends.
    Queued(u32, Vec<Wake>),
    Failed,
}

struct Entry {
    bytes: Arc<[u8]>,
    format: ImageFormat,
    width: u32,
    height: u32,
    used: u64,
    decoded: Option<Arc<Decoded>>,
    work: Work,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<Hash, Entry>,
    /// What each plugin's ids stand for, and when each was registered. A plugin's ids are its own.
    names: HashMap<(String, String), (Hash, u64)>,
    tick: u64,
    bytes: usize,
    decoded_bytes: usize,
    /// Pixels let go of, whose textures the windows drop.
    retired: Vec<Arc<Decoded>>,
}

/// What a page gets to draw for an id.
pub enum Shown {
    /// Not registered, or no longer kept.
    Missing,
    /// It couldn't be decoded.
    Broken { width: u32, height: u32 },
    /// Being decoded: `width` by `height` are the picture's own size.
    Waiting { width: u32, height: u32 },
    Ready { decoded: Arc<Decoded>, width: u32, height: u32 },
}

impl Inner {
    fn add(&mut self, plugin: &str, id: &str, bytes: Vec<u8>) -> Result<(), String> {
        let format = image::guess_format(&bytes)
            .ok()
            .filter(|format| matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP))
            .ok_or("not a png, jpeg, gif or webp")?;
        let (width, height) = image::ImageReader::with_format(Cursor::new(&bytes[..]), format).into_dimensions().map_err(|error| error.to_string())?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err("too large to show".into());
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&Sha256::digest(&bytes));
        self.tick += 1;
        let tick = self.tick;
        match self.entries.get_mut(&hash) {
            Some(entry) => entry.used = tick,
            None => {
                self.bytes += bytes.len();
                self.entries.insert(hash, Entry { bytes: bytes.into(), format, width, height, used: tick, decoded: None, work: Work::Idle });
            }
        }
        if self.names.len() >= MAX_NAMES {
            let mut seen: Vec<u64> = self.names.values().map(|(_, at)| *at).collect();
            seen.sort_unstable();
            let middle = seen[seen.len() / 2];
            self.names.retain(|_, (_, at)| *at > middle);
        }
        self.names.insert((plugin.to_owned(), id.to_owned()), (hash, tick));
        self.evict(&hash);
        Ok(())
    }

    /// Lets go of the least recently used pictures while there are too many or too many bytes, but not `keep`.
    fn evict(&mut self, keep: &Hash) {
        while self.entries.len() > MAX_ENTRIES || self.bytes > KEPT_BYTES {
            let Some(oldest) = self.entries.iter().filter(|(hash, _)| *hash != keep).min_by_key(|(_, entry)| entry.used).map(|(hash, _)| *hash) else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes -= entry.bytes.len();
                self.retire(entry.decoded);
            }
        }
        let entries = &self.entries;
        self.names.retain(|_, (hash, _)| entries.contains_key(hash));
    }

    fn retire(&mut self, decoded: Option<Arc<Decoded>>) {
        if let Some(decoded) = decoded {
            self.decoded_bytes -= decoded.bytes;
            self.retired.push(decoded);
        }
    }
}

fn store() -> &'static Mutex<Inner> {
    static STORE: OnceLock<Mutex<Inner>> = OnceLock::new();
    STORE.get_or_init(Mutex::default)
}

/// Keeps the picture `data` (base64) under `id` for `plugin`'s pages. Refused when it isn't a picture of a kind Claude
/// reads, or is over the limits.
pub fn register(plugin: &str, id: &str, data: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("an image id is up to 64 letters, digits, - and _".into());
    }
    if data.len() > MAX_BYTES / 3 * 4 + 4 {
        return Err("an image is limited to 5 MB".into());
    }
    let bytes = STANDARD.decode(data).map_err(|_| "an image is base64".to_owned())?;
    register_bytes(plugin, id, bytes)
}

/// `register` for bytes the app has decoded from base64 itself.
pub fn register_bytes(plugin: &str, id: &str, bytes: Vec<u8>) -> Result<(), String> {
    if bytes.len() > MAX_BYTES {
        return Err("an image is limited to 5 MB".into());
    }
    store().lock().unwrap().add(plugin, id, bytes)
}

mod decode {
    use std::sync::mpsc;

    use gpui::RenderImage;

    use super::*;

    impl Inner {
        /// What `plugin`'s page draws for `id` at `level` pixels on its longest side. A picture still to decode is queued, and
        /// `wake` is called when it is done.
        pub(super) fn show(&mut self, plugin: &str, id: &str, level: u32, wake: Option<Wake>) -> Shown {
            self.tick += 1;
            let tick = self.tick;
            let Some(hash) = self.names.get(&(plugin.to_owned(), id.to_owned())).map(|(hash, _)| *hash) else {
                return Shown::Missing;
            };
            let Some(entry) = self.entries.get_mut(&hash) else {
                return Shown::Missing;
            };
            entry.used = tick;
            let level = level.clamp(16, MAX_SIDE);
            let (width, height) = (entry.width, entry.height);
            if let Some(decoded) = entry.decoded.as_ref().filter(|decoded| decoded.level >= level) {
                return Shown::Ready { decoded: decoded.clone(), width, height };
            }
            match &mut entry.work {
                Work::Failed => Shown::Broken { width, height },
                Work::Queued(_, waiting) => {
                    waiting.extend(wake);
                    Shown::Waiting { width, height }
                }
                Work::Idle => {
                    entry.work = Work::Queued(level, wake.into_iter().collect());
                    queue(hash);
                    Shown::Waiting { width, height }
                }
            }
        }

        /// A decode ended: keeps its pixels, and returns who waited for it.
        pub(super) fn finish(&mut self, hash: Hash, result: Result<Decoded, String>) -> Vec<Wake> {
            let Some(entry) = self.entries.get_mut(&hash) else {
                return Vec::new();
            };
            let Work::Queued(_, waiting) = std::mem::replace(&mut entry.work, Work::Idle) else {
                return Vec::new();
            };
            let old = match result {
                Ok(decoded) => {
                    self.decoded_bytes += decoded.bytes;
                    entry.decoded.replace(Arc::new(decoded))
                }
                Err(_) => {
                    entry.work = Work::Failed;
                    None
                }
            };
            self.retire(old);
            while self.decoded_bytes > KEPT_DECODED {
                let Some(hash) = self.entries.iter().filter(|(_, entry)| entry.decoded.is_some()).min_by_key(|(_, entry)| entry.used).map(|(hash, _)| *hash) else {
                    break;
                };
                let old = self.entries.get_mut(&hash).and_then(|entry| entry.decoded.take());
                self.retire(old);
            }
            waiting
        }
    }

    /// The texture sources a window has to drop, which the store let go of.
    pub fn take_retired() -> Vec<Arc<Decoded>> {
        std::mem::take(&mut store().lock().unwrap().retired)
    }

    pub fn show(plugin: &str, id: &str, level: u32, wake: Option<Wake>) -> Shown {
        store().lock().unwrap().show(plugin, id, level, wake)
    }

    fn queue(hash: Hash) {
        static JOBS: OnceLock<mpsc::Sender<Hash>> = OnceLock::new();
        let jobs = JOBS.get_or_init(|| {
            let (jobs, queued) = mpsc::channel::<Hash>();
            // One at a time, so a page full of pictures never decodes them all at once.
            let _ = std::thread::Builder::new().name("page-images".into()).spawn(move || queued.into_iter().for_each(work));
            jobs
        });
        let _ = jobs.send(hash);
    }

    fn work(hash: Hash) {
        let job = {
            let inner = store().lock().unwrap();
            inner.entries.get(&hash).and_then(|entry| match entry.work {
                Work::Queued(level, _) => Some((entry.bytes.clone(), entry.format, level, entry.width.max(entry.height))),
                _ => None,
            })
        };
        let Some((bytes, format, level, longest)) = job else {
            return;
        };
        let result = decode(&bytes, format, level, longest);
        let waiting = store().lock().unwrap().finish(hash, result);
        for wake in waiting {
            wake();
        }
    }

    /// The picture as BGRA, which a texture takes, scaled down to `level` pixels on its longest side when it is longer.
    pub fn decode(bytes: &[u8], format: ImageFormat, level: u32, longest: u32) -> Result<Decoded, String> {
        let mut picture = image::ImageReader::with_format(Cursor::new(bytes), format).decode().map_err(|error| error.to_string())?;
        if longest > level {
            picture = picture.thumbnail(level, level);
        }
        let mut pixels = picture.into_rgba8();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        let size = pixels.len();
        Ok(Decoded { image: Arc::new(RenderImage::new([image::Frame::new(pixels)])), level: if longest > level { level } else { u32::MAX }, bytes: size })
    }
}
pub use decode::{decode, show, take_retired};

#[cfg(test)]
mod tests;
