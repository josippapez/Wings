//! Turns text into vectors for semantic search of past sessions (`wings_agents::semantic`): bge-small-en-v1.5 in 8 bits,
//! run by ONNX Runtime on one thread at utility priority, one text at a time. The app starts it with the model's files,
//! sends a JSON line per text (`{"text": .., "query": ..}`) and reads a line back: the unit vector as a scale and 384
//! int8 values (`{"s": .., "v": [..]}`), or `{"error": ..}`. It ends when its stdin closes.
//!
//! The graph optimisations are off and inputs go one at a time: with them on, one 512-token text peaked at 1,035 MB, and
//! a batch of 16 ran a Mac out of memory; off, it peaks at about 160 MB and runs faster (measured on an M1 Pro).

use std::{
    io::{BufRead, Write},
    path::Path,
};

use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};
use serde::Deserialize;
use tokenizers::{Tokenizer, TruncationParams};

const DIM: usize = 384;
/// bge-small's context.
const MAX_TOKENS: usize = 512;
/// What bge-small wants before a query, and only a query.
const QUERY_PREFIX: &str = "Represent this sentence for searching relevant passages: ";

struct Embedder {
    session: Session,
    tokenizer: Tokenizer,
}

impl Embedder {
    fn load(model: &Path, tokenizer: &Path) -> Result<Self, String> {
        let err = |e: &dyn std::fmt::Display| e.to_string();
        let env = ort::init().build().map_err(|e| err(&e))?;
        let session = Session::builder(&env)
            .map_err(|e| err(&e))?
            .with_optimization_level(GraphOptimizationLevel::Disable)
            .map_err(|e| err(&e))?
            .with_intra_threads(1)
            .map_err(|e| err(&e))?
            .with_inter_threads(1)
            .map_err(|e| err(&e))?
            .commit_from_file(model)
            .map_err(|e| err(&e))?;
        let mut tokenizer = Tokenizer::from_file(tokenizer).map_err(|e| err(&e))?;
        tokenizer
            .with_truncation(Some(TruncationParams { max_length: MAX_TOKENS, ..Default::default() }))
            .map_err(|e| err(&e))?;
        tokenizer.with_padding(None);
        Ok(Self { session, tokenizer })
    }

    /// `text`'s mean-pooled unit vector.
    fn embed(&mut self, text: &str) -> Result<Vec<f32>, String> {
        let encoding = self.tokenizer.encode(text, true).map_err(|e| e.to_string())?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&m| m as i64).collect();
        let shape = vec![1i64, ids.len() as i64];
        let types = vec![0i64; ids.len()];
        let tensor = |values: Vec<i64>| Tensor::from_array((shape.clone(), values)).map_err(|e| e.to_string());
        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => tensor(ids)?,
                "attention_mask" => tensor(mask.clone())?,
                "token_type_ids" => tensor(types)?,
            ])
            .map_err(|e| e.to_string())?;
        let (_, hidden) = outputs["last_hidden_state"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let mut mean = vec![0f32; DIM];
        let mut count = 0f32;
        for (token, &on) in mask.iter().enumerate() {
            if on == 1 {
                for (sum, x) in mean.iter_mut().zip(&hidden[token * DIM..(token + 1) * DIM]) {
                    *sum += x;
                }
                count += 1.0;
            }
        }
        let norm = mean.iter().map(|x| (x / count).powi(2)).sum::<f32>().sqrt();
        Ok(mean.into_iter().map(|x| x / count / norm).collect())
    }
}

/// The unit vector `v` as one scale and int8 values, as the index keeps it.
fn quantize(v: &[f32]) -> (f32, Vec<i8>) {
    let most = v.iter().fold(0f32, |m, x| m.max(x.abs()));
    let scale = if most > 0.0 { most / 127.0 } else { 1.0 };
    (scale, v.iter().map(|x| (x / scale).round().clamp(-127.0, 127.0) as i8).collect())
}

#[derive(Deserialize)]
struct Request {
    text: String,
    #[serde(default)]
    query: bool,
}

/// Answers each line of `input` on `output` until `input` ends.
fn serve(embedder: &mut Embedder, input: impl BufRead, mut output: impl Write) {
    for line in input.lines() {
        let Ok(line) = line else { break };
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                let text = if request.query { format!("{QUERY_PREFIX}{}", request.text) } else { request.text };
                match embedder.embed(&text) {
                    Ok(v) => {
                        let (s, v) = quantize(&v);
                        serde_json::json!({ "s": s, "v": v })
                    }
                    Err(error) => serde_json::json!({ "error": error }),
                }
            }
            Err(error) => serde_json::json!({ "error": error.to_string() }),
        };
        if writeln!(output, "{reply}").and_then(|()| output.flush()).is_err() {
            break;
        }
    }
}

/// Runs this thread, which does all the work, at utility priority: below what the app draws with, and not background,
/// which the system starves on a busy Mac (measured: 0.2 messages a second against 12 at utility).
fn utility_priority() {
    #[cfg(target_os = "macos")]
    // SAFETY: sets this thread's own class, with no pointers involved.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

fn main() {
    utility_priority();
    let args: Vec<String> = std::env::args().collect();
    let path = |flag: &str| args.iter().position(|a| a == flag).and_then(|at| args.get(at + 1)).map(Path::new);
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let (Some(model), Some(tokenizer)) = (path("--model"), path("--tokenizer")) else {
        let _ = writeln!(output, "{}", serde_json::json!({ "error": "usage: wings-embed --model PATH --tokenizer PATH" }));
        std::process::exit(2);
    };
    let mut embedder = match Embedder::load(model, tokenizer) {
        Ok(embedder) => embedder,
        Err(error) => {
            let _ = writeln!(output, "{}", serde_json::json!({ "error": format!("Couldn't load the search model: {error}") }));
            std::process::exit(1);
        }
    };
    let _ = writeln!(output, "{}", serde_json::json!({ "ready": true }));
    let _ = output.flush();
    serve(&mut embedder, std::io::stdin().lock(), output);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model's files where repo-docs or Wings keeps them on this Mac. The tests need them, and say so when absent.
    fn model() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        let home = std::env::home_dir()?;
        let mut places: Vec<std::path::PathBuf> = std::fs::read_dir(&home)
            .ok()?
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".claude"))
            .map(|e| e.path().join("models/Xenova/bge-small-en-v1.5"))
            .collect();
        places.push(home.join("Library/Application Support/dev.wings.app/semantic/model"));
        places.into_iter().find_map(|dir| {
            let (model, tokenizer) = [dir.join("onnx/model_quantized.onnx"), dir.join("model_quantized.onnx")]
                .into_iter()
                .find(|p| p.exists())
                .map(|m| (m, dir.join("tokenizer.json")))?;
            tokenizer.exists().then_some((model, tokenizer))
        })
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn similar_texts_get_close_unit_vectors_and_others_do_not() {
        let Some((model, tokenizer)) = model() else {
            eprintln!("skipped: no bge-small-en-v1.5 model on this Mac");
            return;
        };
        let mut e = Embedder::load(&model, &tokenizer).unwrap();
        let cat = e.embed("The cat sits on the mat").unwrap();
        let kitten = e.embed("A kitten is sitting on a rug").unwrap();
        let stocks = e.embed("Stock prices fell sharply this morning").unwrap();
        assert_eq!(cat.len(), DIM);
        assert!((cosine(&cat, &cat) - 1.0).abs() < 1e-4, "a unit vector");
        assert!(cosine(&cat, &kitten) > cosine(&cat, &stocks) + 0.15, "{} vs {}", cosine(&cat, &kitten), cosine(&cat, &stocks));
        let (scale, q) = quantize(&cat);
        let back: Vec<f32> = q.iter().map(|&x| x as f32 * scale).collect();
        assert!(cosine(&cat, &back) / cosine(&back, &back).sqrt() > 0.999, "int8 keeps the direction");
    }

    #[test]
    fn a_long_text_stays_under_300_mb() {
        if std::env::var_os("WINGS_EMBED_ALONE").is_none() {
            // In a process of its own, so the models the other tests load don't count.
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "tests::a_long_text_stays_under_300_mb", "--nocapture"])
                .env("WINGS_EMBED_ALONE", "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let Some((model, tokenizer)) = model() else {
            eprintln!("skipped: no bge-small-en-v1.5 model on this Mac");
            return;
        };
        let mut e = Embedder::load(&model, &tokenizer).unwrap();
        // Past the 512-token context, cut there.
        let long = "the quick brown fox jumps over the lazy dog while the panes keep their history ".repeat(120);
        for _ in 0..5 {
            e.embed(&long).unwrap();
        }
        let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
        // SAFETY: fills the struct given.
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        // In bytes on macOS.
        let peak_mb = usage.ru_maxrss as f64 / (1024.0 * 1024.0);
        println!("[measure] peak memory after five 512-token texts: {peak_mb:.0} MB");
        assert!(peak_mb < 300.0, "peak {peak_mb:.0} MB");
    }

    #[test]
    fn each_line_gets_its_answer_and_a_bad_line_an_error() {
        let Some((model, tokenizer)) = model() else {
            eprintln!("skipped: no bge-small-en-v1.5 model on this Mac");
            return;
        };
        let mut e = Embedder::load(&model, &tokenizer).unwrap();
        let input = b"{\"text\":\"where are panes saved\",\"query\":true}\nnot json\n{\"text\":\"panes save in saved.rs\"}\n";
        let mut output = Vec::new();
        serve(&mut e, &input[..], &mut output);
        let lines: Vec<serde_json::Value> =
            String::from_utf8(output).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["v"].as_array().unwrap().len(), DIM);
        assert!(lines[1]["error"].is_string());
        assert!(lines[2]["s"].as_f64().unwrap() > 0.0);
    }
}
