//! The `record` command's recordings of a Chromium page: its screencast frames (JPEG, as Chromium sends them when
//! something changed) go to a thread that decodes each and writes it with AVFoundation as an H.264 .mp4 that QuickTime
//! plays, at the time Chromium stamped it. A page's recording ends when it is stopped or cancelled, after `maxMs`, or when
//! its page closes; the thread is the one clock, so a page that draws nothing more still ends on time.

use std::{
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use block2::RcBlock;
use image::RgbaImage;
use objc2::{AnyThread, rc::autoreleasepool, rc::Retained, runtime::AnyObject};
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeVideo, AVVideoCodecKey, AVVideoCodecTypeH264,
    AVVideoHeightKey, AVVideoWidthKey,
};
use objc2_core_foundation::CFRetained;
use objc2_core_media::CMTime;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use serde_json::{Value, json};

/// What hears how a recording ended, once.
pub type Waiter = Box<dyn FnOnce(Result<Value, String>) + Send>;

/// How many frames wait for the thread that writes them: a page that changes faster than it writes drops the extra.
const QUEUE: usize = 64;

/// How often the thread looks at whether it was told to end, and whether its time is up.
const TICK: Duration = Duration::from_millis(100);

/// When each frame goes in the file, from when Chromium made it: the first at zero, the next no sooner than a frame at
/// `fps` after it, so a page that changes faster than that is written at that rate.
pub struct Timeline {
    gap_ms: i64,
    first: Option<f64>,
    last: Option<i64>,
}

#[derive(Debug, PartialEq)]
pub enum Offer {
    /// It goes at this many milliseconds.
    Take(i64),
    /// It came too soon after the last: kept in case nothing comes after it, which would leave the page in the file as it
    /// was before.
    Hold,
}

impl Timeline {
    pub fn new(fps: u32) -> Self {
        Self { gap_ms: (1000 / i64::from(fps.max(1))).max(1), first: None, last: None }
    }

    /// A frame made at `at_ms` (any clock, only its differences count).
    pub fn offer(&mut self, at_ms: f64) -> Offer {
        let first = *self.first.get_or_insert(at_ms);
        let pts = ((at_ms - first).round() as i64).max(0);
        match self.last {
            Some(last) if pts - last < self.gap_ms => Offer::Hold,
            _ => {
                self.last = Some(pts);
                Offer::Take(pts)
            }
        }
    }

    /// When a frame that was held goes, once the recording ends: when it came, or a frame after the last.
    pub fn release(&mut self, at_ms: f64) -> i64 {
        let pts = ((at_ms - self.first.unwrap_or(at_ms)).round() as i64).max(0).max(self.last.map_or(0, |last| last + self.gap_ms));
        self.last = Some(pts);
        pts
    }

    /// Where the file ends: a frame after the last one.
    pub fn end(&self) -> i64 {
        self.last.map_or(0, |last| last + self.gap_ms)
    }
}

/// An .mp4 being written. Made and used on one thread: AVFoundation's objects aren't sent between them.
struct Mp4 {
    writer: Retained<AVAssetWriter>,
    input: Retained<AVAssetWriterInput>,
    adaptor: Retained<AVAssetWriterInputPixelBufferAdaptor>,
    width: u32,
    height: u32,
}

/// Where a millisecond is in the file's time.
fn at(ms: i64) -> CMTime {
    // SAFETY: Plain numbers.
    unsafe { CMTime::new(ms, 1000) }
}

impl Mp4 {
    /// A file `width` by `height` pixels, written over `path` if there is one.
    fn create(path: &Path, width: u32, height: u32) -> Result<Self, String> {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
        // SAFETY: AVFoundation's constants and calls with the objects they take, on this thread.
        unsafe {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            let file_type = AVFileTypeMPEG4.ok_or("AVFoundation has no .mp4 type")?;
            let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type).map_err(|error| error.localizedDescription().to_string())?;
            let (codec, codec_key, width_key, height_key) = (AVVideoCodecTypeH264.ok_or("no H.264")?, AVVideoCodecKey.ok_or("no codec key")?, AVVideoWidthKey.ok_or("no width key")?, AVVideoHeightKey.ok_or("no height key")?);
            let (w, h) = (NSNumber::new_u32(width), NSNumber::new_u32(height));
            let (codec, w, h): (&AnyObject, &AnyObject, &AnyObject) = (codec, &w, &h);
            let settings = NSDictionary::from_slices(&[codec_key, width_key, height_key], &[codec, w, h]);
            let input = AVAssetWriterInput::initWithMediaType_outputSettings(AVAssetWriterInput::alloc(), AVMediaTypeVideo.ok_or("no video type")?, Some(&settings));
            let adaptor = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&input, None);
            if !writer.canAddInput(&input) {
                return Err("The .mp4 writer takes no video input".into());
            }
            writer.addInput(&input);
            if !writer.startWriting() {
                return Err(Self::failure(&writer, "The .mp4 writer didn't start"));
            }
            writer.startSessionAtSourceTime(at(0));
            Ok(Self { writer, input, adaptor, width, height })
        }
    }

    fn failure(writer: &AVAssetWriter, fallback: &str) -> String {
        // SAFETY: Reads the writer's error.
        unsafe { writer.error() }.map_or_else(|| fallback.to_owned(), |error| error.localizedDescription().to_string())
    }

    /// Writes `frame`, which is this file's size, at `ms`.
    fn append(&mut self, frame: &RgbaImage, ms: i64) -> Result<(), String> {
        debug_assert_eq!(frame.dimensions(), (self.width, self.height));
        let (width, height) = (self.width as usize, self.height as usize);
        let mut raw = std::ptr::null_mut();
        // SAFETY: A buffer of this format and size, whose output pointer is writable.
        let made = unsafe { CVPixelBufferCreate(None, width, height, kCVPixelFormatType_32BGRA, None, NonNull::from(&mut raw)) };
        if made != kCVReturnSuccess {
            return Err(format!("A frame of {width}x{height} wasn't made: status {made}"));
        }
        // SAFETY: A successful create hands over this buffer.
        let buffer: CFRetained<CVPixelBuffer> = unsafe { CFRetained::from_raw(NonNull::new(raw).ok_or("no frame buffer")?) };
        let flags = CVPixelBufferLockFlags(0);
        // SAFETY: Locked while it is written, which is within its rows: `bytes_per_row` apart, each `width` pixels.
        unsafe {
            if CVPixelBufferLockBaseAddress(&buffer, flags) != kCVReturnSuccess {
                return Err("A frame couldn't be locked".into());
            }
            let (base, stride) = (CVPixelBufferGetBaseAddress(&buffer).cast::<u8>(), CVPixelBufferGetBytesPerRow(&buffer));
            for (y, row) in frame.rows().enumerate() {
                let line = std::slice::from_raw_parts_mut(base.add(y * stride), width * 4);
                for (out, pixel) in line.chunks_exact_mut(4).zip(row) {
                    out.copy_from_slice(&[pixel.0[2], pixel.0[1], pixel.0[0], pixel.0[3]]);
                }
            }
            CVPixelBufferUnlockBaseAddress(&buffer, flags);
        }
        // The writer takes as fast as it encodes: wait for it.
        let started = Instant::now();
        // SAFETY: The input's own state.
        while !unsafe { self.input.isReadyForMoreMediaData() } {
            if started.elapsed() > Duration::from_secs(20) || unsafe { self.writer.status() } == AVAssetWriterStatus::Failed {
                return Err(Self::failure(&self.writer, "The .mp4 writer stopped taking frames"));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        // SAFETY: The buffer is in the format the writer takes, and its times only go up.
        if unsafe { self.adaptor.appendPixelBuffer_withPresentationTime(&buffer, at(ms)) } {
            Ok(())
        } else {
            Err(Self::failure(&self.writer, "A frame wasn't written"))
        }
    }

    /// Ends the file `end_ms` into it, and waits until it is written.
    fn finish(self, end_ms: i64) -> Result<(), String> {
        let (done, wait) = mpsc::channel();
        // SAFETY: The calls end a session this thread started, and the block only sends.
        unsafe {
            self.input.markAsFinished();
            self.writer.endSessionAtSourceTime(at(end_ms));
            self.writer.finishWritingWithCompletionHandler(&RcBlock::new(move || {
                let _ = done.send(());
            }));
        }
        wait.recv_timeout(Duration::from_secs(60)).map_err(|_| "The .mp4 wasn't finished in a minute".to_owned())?;
        // SAFETY: The writer's own state.
        if unsafe { self.writer.status() } == AVAssetWriterStatus::Completed { Ok(()) } else { Err(Self::failure(&self.writer, "The .mp4 wasn't written")) }
    }

    /// Drops what was written.
    fn cancel(self) {
        // SAFETY: Ends the writer, which removes its file.
        unsafe { self.writer.cancelWriting() }
    }
}

/// `frame` as a picture of the file's size: cut to even sides, which the encoder takes, if it is the first.
fn fitted(frame: RgbaImage, size: Option<(u32, u32)>) -> RgbaImage {
    match size {
        Some((width, height)) if frame.dimensions() != (width, height) => image::imageops::resize(&frame, width, height, image::imageops::FilterType::Triangle),
        Some(_) => frame,
        None => {
            let (width, height) = ((frame.width() & !1).max(2), (frame.height() & !1).max(2));
            if frame.dimensions() == (width, height) { frame } else { image::imageops::crop_imm(&frame, 0, 0, width.min(frame.width()), height.min(frame.height())).to_image() }
        }
    }
}

enum Msg {
    Frame { jpeg: Vec<u8>, at_ms: f64 },
    /// Looks at what it was told.
    Wake,
}

#[derive(Clone, Copy, PartialEq)]
enum Want {
    Finish,
    Cancel,
}

/// What the thread and the page share.
struct Progress {
    frames: u64,
    want: Option<Want>,
    /// Why it ended when it wasn't asked to: `reached maxMs`, the page closed.
    because: Option<String>,
    /// How it ended, once it has, with how long the file is.
    outcome: Option<(Result<Value, String>, i64)>,
    /// Hears it end, once, if it hasn't.
    waiter: Option<Waiter>,
}

/// One page's recording. Its frames go to a thread of its own.
pub struct Recording {
    tx: mpsc::SyncSender<Msg>,
    state: Arc<Mutex<Progress>>,
    pub path: PathBuf,
    started: Instant,
}

impl Recording {
    /// Starts writing to `path`, one the file rules let it (`policy::Files::output`, `prepare`) in the profile whose web
    /// folder is `web`, written aside and renamed into place when it is done, at most `fps` frames a second and for
    /// at most `max_ms`.
    pub fn start(web: PathBuf, path: PathBuf, fps: u32, max_ms: u64) -> Self {
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let state = Arc::new(Mutex::new(Progress { frames: 0, want: None, because: None, outcome: None, waiter: None }));
        let started = Instant::now();
        let (file, shared) = (path.clone(), state.clone());
        std::thread::spawn(move || work(&web, &file, fps, started + Duration::from_millis(max_ms), &rx, &shared));
        Self { tx, state, path, started }
    }

    /// A frame Chromium made at `at_ms`. One that finds the queue full is dropped.
    pub fn frame(&self, jpeg: Vec<u8>, at_ms: f64) {
        let _ = self.tx.try_send(Msg::Frame { jpeg, at_ms });
    }

    /// The milliseconds since it started, for a frame that comes without a time.
    pub fn now_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    /// Whether it ended, by itself or as it was asked.
    pub fn ended(&self) -> bool {
        self.state.lock().unwrap().outcome.is_some()
    }

    /// Ends it, as `because` says (a recording that was asked to stop says nothing): its status says so from then on.
    pub fn end(&self, because: &str) {
        let mut state = self.state.lock().unwrap();
        if state.want.is_none() && state.outcome.is_none() {
            state.because.get_or_insert_with(|| because.to_owned());
            state.want = Some(Want::Finish);
        }
        drop(state);
        let _ = self.tx.try_send(Msg::Wake);
    }

    /// What `status` answers.
    pub fn status(&self) -> Value {
        let state = self.state.lock().unwrap();
        match &state.outcome {
            None => json!({ "recording": true, "durationMs": self.started.elapsed().as_millis() as u64, "frames": state.frames }),
            Some((_, duration)) => {
                let mut answer = json!({ "recording": false, "durationMs": duration, "frames": state.frames, "path": self.path });
                if let Some(because) = &state.because {
                    answer["endedBecause"] = json!(because);
                }
                answer
            }
        }
    }

    /// Stops it and, once the file is written, tells `waiter` its answer. One that ended already tells at once.
    pub fn stop(self, waiter: Waiter) {
        let mut state = self.state.lock().unwrap();
        if let Some((outcome, _)) = state.outcome.take() {
            let because = state.because.clone();
            drop(state);
            return waiter(outcome.map(|mut answer| {
                if let Some(because) = because {
                    answer["endedBecause"] = json!(because);
                }
                answer
            }));
        }
        state.waiter = Some(waiter);
        state.want = Some(Want::Finish);
        drop(state);
        let _ = self.tx.try_send(Msg::Wake);
    }

    /// Drops it and its file.
    pub fn cancel(self) {
        self.state.lock().unwrap().want = Some(Want::Cancel);
        let _ = self.tx.try_send(Msg::Wake);
    }
}

impl Drop for Recording {
    /// A recording nobody ended is dropped with its file.
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap();
        if state.outcome.is_none() && state.want.is_none() {
            state.want = Some(Want::Cancel);
            drop(state);
            let _ = self.tx.try_send(Msg::Wake);
        }
    }
}

/// The thread's work: frames in, a file out, and how it went in `shared`.
fn work(web: &Path, path: &Path, fps: u32, deadline: Instant, rx: &mpsc::Receiver<Msg>, shared: &Mutex<Progress>) {
    let files = super::policy::Files::of(web);
    let partial = files.partial(path, "mp4");
    let mut line = Timeline::new(fps);
    let mut mp4: Option<Mp4> = None;
    let mut held: Option<(Vec<u8>, f64)> = None;
    let mut failed: Option<String> = None;
    let write = |jpeg: &[u8], pts: i64, mp4: &mut Option<Mp4>| -> Result<(), String> {
        let frame = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).map_err(|error| error.to_string())?.into_rgba8();
        autoreleasepool(|_| {
            let size = mp4.as_ref().map(|mp4| (mp4.width, mp4.height));
            let frame = fitted(frame, size);
            let file = match mp4 {
                Some(file) => file,
                None => mp4.insert(Mp4::create(&partial, frame.width(), frame.height())?),
            };
            file.append(&frame, pts)
        })?;
        shared.lock().unwrap().frames += 1;
        Ok(())
    };
    let want = loop {
        if let Some(want) = shared.lock().unwrap().want {
            break want;
        }
        if Instant::now() >= deadline {
            let mut state = shared.lock().unwrap();
            state.because.get_or_insert_with(|| "it reached maxMs".to_owned());
            break Want::Finish;
        }
        match rx.recv_timeout(TICK) {
            Ok(Msg::Frame { jpeg, at_ms }) if failed.is_none() => match line.offer(at_ms) {
                Offer::Take(pts) => {
                    held = None;
                    failed = write(&jpeg, pts, &mut mp4).err();
                }
                Offer::Hold => held = Some((jpeg, at_ms)),
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => break Want::Cancel,
            _ => {}
        }
    };
    // What is queued still goes in, for a recording that ends as it should.
    if want == Want::Finish {
        while let Ok(Msg::Frame { jpeg, at_ms }) = rx.try_recv() {
            if failed.is_some() {
                break;
            }
            match line.offer(at_ms) {
                Offer::Take(pts) => {
                    held = None;
                    failed = write(&jpeg, pts, &mut mp4).err();
                }
                Offer::Hold => held = Some((jpeg, at_ms)),
            }
        }
        if let (Some((jpeg, at_ms)), None) = (held.take(), &failed) {
            let pts = line.release(at_ms);
            failed = write(&jpeg, pts, &mut mp4).err();
        }
    }
    let end = line.end();
    let frames = shared.lock().unwrap().frames;
    let outcome = match (want, failed, mp4) {
        (Want::Cancel, _, mp4) => {
            mp4.into_iter().for_each(Mp4::cancel);
            Err("The recording was cancelled".to_owned())
        }
        (_, Some(error), mp4) => {
            mp4.into_iter().for_each(Mp4::cancel);
            Err(error)
        }
        (_, None, None) => Err("The page drew no frame while it was recorded".to_owned()),
        (_, None, Some(file)) => {
            let (width, height) = (file.width, file.height);
            file.finish(end).and_then(|()| files.commit(&partial, path)).and_then(|()| {
                let bytes = std::fs::metadata(path).map_err(|error| error.to_string())?.len();
                Ok(json!({ "path": path, "bytes": bytes, "durationMs": end, "width": width, "height": height, "frames": frames }))
            })
        }
    };
    if outcome.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    let mut state = shared.lock().unwrap();
    match state.waiter.take() {
        Some(waiter) => {
            let because = state.because.clone();
            drop(state);
            waiter(outcome.map(|mut answer| {
                if let Some(because) = because {
                    answer["endedBecause"] = json!(because);
                }
                answer
            }));
            // Reported: nothing is left to ask about.
            shared.lock().unwrap().outcome = Some((Err("reported".into()), end));
        }
        None => state.outcome = Some((outcome, end)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JPEG of a flat color, as Chromium's frames are.
    fn jpeg(width: u32, height: u32, shade: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([shade, 255 - shade, 90]));
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90).encode_image(&image).unwrap();
        bytes
    }

    /// The length an .mp4 says it has, in milliseconds, from its `mvhd` box.
    fn mp4_duration_ms(bytes: &[u8]) -> u64 {
        let at = bytes.windows(4).position(|window| window == b"mvhd").expect("a movie header") + 4;
        assert_eq!(bytes[at], 0, "a version 0 header");
        let word = |from: usize| u64::from(u32::from_be_bytes(bytes[at + from..at + from + 4].try_into().unwrap()));
        word(16) * 1000 / word(12)
    }

    #[test]
    fn frames_are_placed_at_their_own_time_and_no_closer_than_the_rate() {
        let mut line = Timeline::new(10);
        assert_eq!(line.offer(5000.0), Offer::Take(0), "the first is at zero, whenever Chromium made it");
        assert_eq!(line.offer(5030.0), Offer::Hold, "a tenth of a second at ten frames a second");
        assert_eq!(line.offer(5100.0), Offer::Take(100));
        assert_eq!(line.offer(5150.0), Offer::Hold);
        assert_eq!(line.offer(5400.0), Offer::Take(400), "a gap in the page stays a gap");
        assert_eq!(line.end(), 500);
        // The frame held back when nothing came after it goes a frame after the last at the soonest.
        assert_eq!(line.offer(5410.0), Offer::Hold);
        assert_eq!(line.release(5410.0), 500);
        assert_eq!(line.end(), 600);
        assert_eq!(Timeline::new(60).offer(0.0), Offer::Take(0));
        assert_eq!(Timeline::new(60).end(), 0, "nothing was written");
        let mut backwards = Timeline::new(30);
        assert_eq!(backwards.offer(1000.0), Offer::Take(0));
        assert_eq!(backwards.offer(900.0), Offer::Hold, "a frame from before the first is never earlier than it");
    }

    #[test]
    fn the_first_frame_sets_even_sides_and_later_ones_are_brought_to_them() {
        let odd = fitted(RgbaImage::new(101, 51), None);
        assert_eq!(odd.dimensions(), (100, 50));
        assert_eq!(fitted(RgbaImage::new(64, 48), None).dimensions(), (64, 48));
        assert_eq!(fitted(RgbaImage::new(80, 40), Some((64, 48))).dimensions(), (64, 48));
    }

    #[test]
    fn three_frames_are_written_as_an_mp4_of_the_times_they_came() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deeper").join("out.mp4");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let recording = Recording::start(dir.path().join("web"), path.clone(), 10, 60_000);
        recording.frame(jpeg(64, 48, 10), 1000.0);
        recording.frame(jpeg(64, 48, 120), 1500.0);
        recording.frame(jpeg(64, 48, 240), 2000.0);
        let (told, heard) = mpsc::channel();
        recording.stop(Box::new(move |answer| told.send(answer).unwrap()));
        let answer = heard.recv_timeout(Duration::from_secs(60)).unwrap().unwrap();
        assert_eq!((answer["width"].as_u64(), answer["height"].as_u64(), answer["frames"].as_u64(), answer["durationMs"].as_u64()), (Some(64), Some(48), Some(3), Some(1100)), "{answer}");
        assert!(answer.get("endedBecause").is_none());
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(answer["bytes"].as_u64(), Some(bytes.len() as u64));
        assert_eq!(&bytes[4..8], b"ftyp", "an MP4 file");
        assert!(bytes.windows(4).any(|window| window == b"avc1"), "H.264");
        let length = mp4_duration_ms(&bytes);
        assert!((1000..=1200).contains(&length), "{length} ms for frames that came over a second");
    }

    #[test]
    fn a_recording_that_ran_out_of_time_says_so_and_one_without_frames_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.mp4");
        let recording = Recording::start(dir.path().join("web"), path.clone(), 30, 300);
        recording.frame(jpeg(32, 32, 50), 0.0);
        std::thread::sleep(Duration::from_millis(1200));
        assert!(recording.ended());
        let status = recording.status();
        assert_eq!((status["recording"].as_bool(), status["frames"].as_u64()), (Some(false), Some(1)));
        assert_eq!(status["endedBecause"].as_str(), Some("it reached maxMs"));
        let (told, heard) = mpsc::channel();
        recording.stop(Box::new(move |answer| told.send(answer).unwrap()));
        let answer = heard.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        assert_eq!(answer["endedBecause"].as_str(), Some("it reached maxMs"));
        assert!(path.exists());
        let empty = Recording::start(dir.path().join("web"), dir.path().join("none.mp4"), 30, 60_000);
        let (told, heard) = mpsc::channel();
        empty.stop(Box::new(move |answer| told.send(answer).unwrap()));
        assert!(heard.recv_timeout(Duration::from_secs(5)).unwrap().unwrap_err().contains("no frame"));
        assert!(!dir.path().join("none.mp4").exists());
    }

    #[test]
    fn a_cancelled_recording_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone.mp4");
        let recording = Recording::start(dir.path().join("web"), path.clone(), 30, 60_000);
        recording.frame(jpeg(32, 32, 50), 0.0);
        std::thread::sleep(Duration::from_millis(500));
        recording.cancel();
        std::thread::sleep(Duration::from_millis(800));
        assert!(!path.exists());
    }
}
