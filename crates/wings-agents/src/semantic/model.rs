//! The model semantic search embeds with, fetched once into the app's folder, and the helper that runs it
//! (`wings-embed`, beside the app's own binary). The model is bge-small-en-v1.5 in 8 bits as Xenova publishes it, the one
//! repo-docs settled on after measuring it against larger ones. Pinned to a revision and checked by SHA-256: a copy repo-docs
//! already downloaded (`~/.claude*/models`) is taken when it is the same file, else it downloads, into a temporary file
//! that becomes the model only once its hash is right.

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::Duration,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::index::DIM;

pub struct File {
    pub name: &'static str,
    url: &'static str,
    sha256: &'static str,
    pub size: u64,
}

const REVISION: &str = "https://huggingface.co/Xenova/bge-small-en-v1.5/resolve/ea104dacec62c0de699686887e3f920caeb4f3e3";

pub const MODEL: File = File {
    name: "model_quantized.onnx",
    url: "onnx/model_quantized.onnx",
    sha256: "6c9c6101a956d62dfb5e7190c538226c0c5bb9cb27b651234b6df063ee7dbfe4",
    size: 34_014_426,
};

pub const TOKENIZER: File = File {
    name: "tokenizer.json",
    url: "tokenizer.json",
    sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
    size: 711_396,
};

/// The model's id in the index's header: the first 8 bytes of its hash.
pub fn model_id() -> u64 {
    u64::from_str_radix(&MODEL.sha256[..16], 16).unwrap_or_default()
}

/// The model's files in `dir`, when both are there.
pub fn present(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let (model, tokenizer) = (dir.join(MODEL.name), dir.join(TOKENIZER.name));
    let size = |path: &Path| fs::metadata(path).map(|m| m.len()).ok();
    (size(&model) == Some(MODEL.size) && size(&tokenizer) == Some(TOKENIZER.size)).then_some((model, tokenizer))
}

/// The model's files in `dir`, fetched first when they aren't there: from repo-docs' copy in `homes` when it is the same
/// file, else from Hugging Face.
pub fn ensure(dir: &Path, homes: &[PathBuf]) -> Result<(PathBuf, PathBuf), String> {
    fetch(dir, homes, &[&MODEL, &TOKENIZER], download)?;
    present(dir).ok_or_else(|| "The search model is incomplete".to_owned())
}

/// Each of `files` into `dir` that isn't there yet: copied from the first of `homes` that holds the same file, else
/// `download`ed.
fn fetch(dir: &Path, homes: &[PathBuf], files: &[&File], download: impl Fn(&File, &Path) -> Result<(), String>) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    for file in files {
        let target = dir.join(file.name);
        if fs::metadata(&target).is_ok_and(|m| m.len() == file.size) {
            continue;
        }
        let copied = homes.iter().any(|home| copy_checked(&home.join("Xenova/bge-small-en-v1.5").join(file.url), &target, file).is_ok());
        if !copied {
            download(file, &target)?;
        }
    }
    Ok(())
}

/// `~/.claude*/models`, where repo-docs keeps its models.
pub fn repo_docs_models() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    let mut found: Vec<PathBuf> = fs::read_dir(&home)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".claude"))
        .map(|e| e.path().join("models"))
        .filter(|p| p.is_dir())
        .collect();
    found.sort();
    found
}

fn copy_checked(from: &Path, to: &Path, file: &File) -> Result<(), String> {
    let source = fs::File::open(from).map_err(|e| e.to_string())?;
    write_checked(source, to, file)
}

fn download(file: &File, to: &Path) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into();
    let url = format!("{REVISION}/{}", file.url);
    let mut response = agent.get(&url).call().map_err(|e| format!("Couldn't download the search model: {e}"))?;
    write_checked(response.body_mut().as_reader(), to, file)
}

/// Writes `source` to a temporary file beside `to`, which becomes `to` only when its size and hash are `file`'s.
fn write_checked(mut source: impl Read, to: &Path, file: &File) -> Result<(), String> {
    let part = to.with_extension("part");
    let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let (mut buf, mut total) = (vec![0u8; 1 << 16], 0u64);
    let result = loop {
        let n = match source.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => n,
            Err(e) => break Err(e.to_string()),
        };
        total += n as u64;
        if total > file.size {
            break Err("larger than expected".into());
        }
        hash.update(&buf[..n]);
        if let Err(e) = out.write_all(&buf[..n]) {
            break Err(e.to_string());
        }
    };
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let checked = result.and_then(|()| {
        if total == file.size && digest == file.sha256 { Ok(()) } else { Err(format!("{} isn't the file it should be", file.name)) }
    });
    match checked.and_then(|()| out.sync_all().map_err(|e| e.to_string())).and_then(|()| fs::rename(&part, to).map_err(|e| e.to_string())) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&part);
            Err(e)
        }
    }
}

/// What turns text into vectors: the helper process, or a stand-in in tests.
pub trait Embed: Send {
    /// `text`'s unit vector as a scale and int8 values. A query gets the model's retrieval prefix.
    fn embed(&mut self, text: &str, query: bool) -> Result<(f32, [i8; DIM]), String>;
}

/// The helper process, talking JSON lines over its stdin and stdout. It ends when this is dropped: its stdin closes.
pub struct Helper {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

#[derive(Deserialize)]
struct Reply {
    ready: Option<bool>,
    error: Option<String>,
    s: Option<f32>,
    v: Option<Vec<i8>>,
}

impl Helper {
    pub fn spawn(program: &Path, model: &Path, tokenizer: &Path) -> Result<Self, String> {
        let mut child = Command::new(program)
            .arg("--model")
            .arg(model)
            .arg("--tokenizer")
            .arg(tokenizer)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't start {}: {e}", program.display()))?;
        let input = child.stdin.take().ok_or("no stdin")?;
        let output = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut helper = Self { child, input, output };
        match helper.read()? {
            Reply { ready: Some(true), .. } => Ok(helper),
            Reply { error: Some(e), .. } => Err(e),
            _ => Err("The search helper didn't start".into()),
        }
    }

    fn read(&mut self) -> Result<Reply, String> {
        let mut line = String::new();
        match self.output.read_line(&mut line) {
            Ok(0) | Err(_) => Err("The search helper stopped".into()),
            Ok(_) => serde_json::from_str(&line).map_err(|e| e.to_string()),
        }
    }
}

impl Embed for Helper {
    fn embed(&mut self, text: &str, query: bool) -> Result<(f32, [i8; DIM]), String> {
        let request = serde_json::json!({ "text": text, "query": query });
        writeln!(self.input, "{request}").and_then(|()| self.input.flush()).map_err(|_| "The search helper stopped".to_owned())?;
        match self.read()? {
            Reply { s: Some(scale), v: Some(v), .. } if v.len() == DIM => {
                let mut out = [0i8; DIM];
                out.copy_from_slice(&v);
                Ok((scale, out))
            }
            Reply { error: Some(e), .. } => Err(e),
            _ => Err("The search helper answered something else".into()),
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        // Its stdin closes with `self`, which ends it; waiting reaps it.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wings-semantic-model-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const SMALL: File = File {
        name: "small.bin",
        url: "small.bin",
        // SHA-256 of "hello\n".
        sha256: "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03",
        size: 6,
    };

    #[test]
    fn a_file_becomes_the_model_only_when_its_hash_is_right() {
        let dir = temp("checked");
        let to = dir.join("small.bin");
        assert!(write_checked(&b"hello\n"[..], &to, &SMALL).is_ok());
        assert_eq!(fs::read(&to).unwrap(), b"hello\n");
        let wrong = dir.join("wrong.bin");
        assert!(write_checked(&b"hellO\n"[..], &wrong, &SMALL).is_err());
        assert!(!wrong.exists() && !wrong.with_extension("part").exists(), "nothing left behind");
        assert!(write_checked(&b"hello\nmore"[..], &wrong, &SMALL).is_err(), "too long");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repo_docs_copy_is_taken_when_it_is_the_same_file_and_downloaded_otherwise() {
        let (home, dir) = (temp("home"), temp("dir"));
        let copy = home.join("Xenova/bge-small-en-v1.5").join(SMALL.url);
        fs::create_dir_all(copy.parent().unwrap()).unwrap();
        let downloads = std::cell::Cell::new(0);
        let download = |file: &File, to: &Path| {
            downloads.set(downloads.get() + 1);
            write_checked(&b"hello\n"[..], to, file)
        };
        fs::write(&copy, b"hello\n").unwrap();
        fetch(&dir, std::slice::from_ref(&home), &[&SMALL], download).unwrap();
        assert_eq!((fs::read(dir.join(SMALL.name)).unwrap(), downloads.get()), (b"hello\n".to_vec(), 0), "copied");

        fs::remove_file(dir.join(SMALL.name)).unwrap();
        fs::write(&copy, b"other\n").unwrap();
        fetch(&dir, std::slice::from_ref(&home), &[&SMALL], download).unwrap();
        assert_eq!(downloads.get(), 1, "another file: downloaded");
        fetch(&dir, std::slice::from_ref(&home), &[&SMALL], download).unwrap();
        assert_eq!(downloads.get(), 1, "there already: nothing to do");
        fs::remove_dir_all(home).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
