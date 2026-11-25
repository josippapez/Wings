//! The semantic index on disk and in memory: one int8 vector per chunk of a message, with the transcript, the message's
//! number in it and where the chunk sits in its text, never the text itself (a search reads that back from the
//! transcript). And the rules that keep it in step with the transcripts on disk (`plan`).
//!
//! The file is a header, then the transcripts, then the rows, all little-endian. A header that isn't this build's (another
//! format, model, chunk size or width) reads as an empty index, which the indexer builds again from scratch.

use std::path::PathBuf;

/// The model's width: bge-small-en-v1.5's 384.
pub const DIM: usize = 384;
/// The most bytes of a message one vector stands for. bge-small reads at most 512 tokens, and 1,500 bytes of the
/// transcripts measured about 375.
pub const CHUNK_BYTES: usize = 1500;
const MAGIC: [u8; 4] = *b"WSEM";
pub const FORMAT: u32 = 1;
/// Messages with fewer words say too little to find ("yes", "commit it") and would match any short query.
const MIN_WORDS: usize = 3;
/// A session's best chunk loses this much for each e-fold of its chunks, so a session with thousands of them doesn't win
/// on chance (measured on six reworded queries: it moved the right sessions up within the top five).
const LONG_SESSION: f32 = 0.005;

/// What the index was made with. Any difference from this build's makes the indexer start again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Header {
    pub format: u32,
    /// The first 8 bytes of the model file's SHA-256.
    pub model: u64,
    pub chunk: u32,
    pub dim: u32,
}

impl Header {
    pub fn current(model: u64) -> Self {
        Self { format: FORMAT, model, chunk: CHUNK_BYTES as u32, dim: DIM as u32 }
    }
}

/// A transcript as it was when its rows were made.
#[derive(Clone, Debug, PartialEq)]
pub struct Transcript {
    pub path: PathBuf,
    /// The session it belongs to: its own, or the parent's for a subagent's.
    pub session: String,
    pub len: u64,
    pub modified: u64,
    /// The messages it had then.
    pub messages: u32,
}

/// One chunk's vector: a scale and 384 int8 values, whose product is the unit vector.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub transcript: u32,
    pub message: u32,
    /// Where the chunk is in the message's text, in bytes.
    pub start: u32,
    pub len: u32,
    pub scale: f32,
    pub vector: [i8; DIM],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Index {
    pub header: Header,
    pub transcripts: Vec<Transcript>,
    pub rows: Vec<Row>,
}

impl Index {
    pub fn empty(header: Header) -> Self {
        Self { header, transcripts: Vec::new(), rows: Vec::new() }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + self.transcripts.len() * 160 + self.rows.len() * (20 + DIM));
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.header.format.to_le_bytes());
        out.extend_from_slice(&self.header.model.to_le_bytes());
        out.extend_from_slice(&self.header.chunk.to_le_bytes());
        out.extend_from_slice(&self.header.dim.to_le_bytes());
        out.extend_from_slice(&(self.transcripts.len() as u32).to_le_bytes());
        for t in &self.transcripts {
            let path = t.path.to_string_lossy();
            for text in [path.as_bytes(), t.session.as_bytes()] {
                out.extend_from_slice(&(text.len() as u32).to_le_bytes());
                out.extend_from_slice(text);
            }
            out.extend_from_slice(&t.len.to_le_bytes());
            out.extend_from_slice(&t.modified.to_le_bytes());
            out.extend_from_slice(&t.messages.to_le_bytes());
        }
        out.extend_from_slice(&(self.rows.len() as u32).to_le_bytes());
        for r in &self.rows {
            for n in [r.transcript, r.message, r.start, r.len] {
                out.extend_from_slice(&n.to_le_bytes());
            }
            out.extend_from_slice(&r.scale.to_le_bytes());
            out.extend(r.vector.iter().map(|&x| x as u8));
        }
        out
    }

    /// The index in `bytes`, or an empty one when they aren't an index made with `header`, or are cut short.
    pub fn decode(bytes: &[u8], header: Header) -> Self {
        Self::try_decode(bytes, header).unwrap_or_else(|| Self::empty(header))
    }

    fn try_decode(bytes: &[u8], header: Header) -> Option<Self> {
        let mut r = Reader(bytes);
        if r.take(4)? != MAGIC {
            return None;
        }
        let found = Header { format: r.u32()?, model: r.u64()?, chunk: r.u32()?, dim: r.u32()? };
        if found != header {
            return None;
        }
        let count = r.u32()? as usize;
        let mut transcripts = Vec::with_capacity(count.min(1 << 16));
        for _ in 0..count {
            let path = PathBuf::from(r.text()?);
            let session = r.text()?;
            transcripts.push(Transcript { path, session, len: r.u64()?, modified: r.u64()?, messages: r.u32()? });
        }
        let count = r.u32()? as usize;
        let mut rows = Vec::with_capacity(count.min(1 << 20));
        for _ in 0..count {
            let (transcript, message, start, len) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
            let scale = f32::from_le_bytes(r.take(4)?.try_into().ok()?);
            let mut vector = [0i8; DIM];
            for (x, &b) in vector.iter_mut().zip(r.take(DIM)?) {
                *x = b as i8;
            }
            if transcript as usize >= transcripts.len() {
                return None;
            }
            rows.push(Row { transcript, message, start, len, scale, vector });
        }
        r.0.is_empty().then_some(Self { header, transcripts, rows })
    }

    /// The sessions that have rows.
    pub fn sessions(&self) -> usize {
        let mut ids: Vec<&str> = self.transcripts.iter().map(|t| t.session.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    }

    /// Forgets a transcript and its rows.
    pub fn drop_transcript(&mut self, path: &std::path::Path) {
        let Some(at) = self.transcripts.iter().position(|t| t.path == path) else { return };
        self.transcripts.remove(at);
        let at = at as u32;
        self.rows.retain(|r| r.transcript != at);
        for r in &mut self.rows {
            if r.transcript > at {
                r.transcript -= 1;
            }
        }
    }

    /// Takes in what was embedded of `disk` from its message `from` on: its rows from there replace those it had.
    pub fn update(&mut self, disk: &OnDisk, from: u32, messages: u32, rows: Vec<Row>) {
        let at = match self.transcripts.iter().position(|t| t.path == disk.path) {
            Some(at) => at,
            None => {
                self.transcripts.push(Transcript {
                    path: disk.path.clone(),
                    session: disk.session.clone(),
                    len: 0,
                    modified: 0,
                    messages: 0,
                });
                self.transcripts.len() - 1
            }
        };
        self.transcripts[at] =
            Transcript { path: disk.path.clone(), session: disk.session.clone(), len: disk.len, modified: disk.modified, messages };
        let id = at as u32;
        self.rows.retain(|r| r.transcript != id || r.message < from);
        self.rows.extend(rows.into_iter().map(|r| Row { transcript: id, ..r }));
    }

    /// Each session's best chunk for the unit vector `query`, best first: its score, less a little for each e-fold of its
    /// chunks.
    pub fn rank(&self, query: &[f32]) -> Vec<(String, Best)> {
        let mut best: std::collections::HashMap<&str, (Best, usize)> = std::collections::HashMap::new();
        for r in &self.rows {
            let dot: f32 = r.vector.iter().zip(query).map(|(&a, &b)| a as f32 * b).sum::<f32>() * r.scale;
            let session = self.transcripts[r.transcript as usize].session.as_str();
            let hit = Best { transcript: r.transcript, message: r.message, start: r.start, len: r.len, score: dot };
            let entry = best.entry(session).or_insert((hit, 0));
            entry.1 += 1;
            if dot > entry.0.score {
                entry.0 = hit;
            }
        }
        let mut ranked: Vec<(String, Best)> = best
            .into_iter()
            .map(|(session, (mut hit, chunks))| {
                hit.score -= LONG_SESSION * (1.0 + chunks as f32).ln();
                (session.to_owned(), hit)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));
        ranked
    }
}

/// A session's best chunk for a query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Best {
    pub transcript: u32,
    pub message: u32,
    pub start: u32,
    pub len: u32,
    pub score: f32,
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn text(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }
}

/// A transcript as it is on disk now.
#[derive(Clone, Debug, PartialEq)]
pub struct OnDisk {
    pub path: PathBuf,
    pub session: String,
    pub len: u64,
    pub modified: u64,
}

/// What brings the index in step with the transcripts on disk.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// Each transcript to embed, from the message given on.
    pub embed: Vec<(OnDisk, u32)>,
    /// Transcripts that are gone.
    pub gone: Vec<PathBuf>,
}

/// The rules: a new transcript is embedded whole; one that grew from its last message on, as that message may have
/// grown; one that is shorter or was rewritten in place whole again; one that is gone is dropped.
pub fn plan(index: &Index, disk: &[OnDisk]) -> Plan {
    let known: std::collections::HashMap<&std::path::Path, &Transcript> =
        index.transcripts.iter().map(|t| (t.path.as_path(), t)).collect();
    let mut plan = Plan::default();
    for file in disk {
        match known.get(file.path.as_path()) {
            None => plan.embed.push((file.clone(), 0)),
            Some(t) if t.len == file.len && t.modified == file.modified => {}
            Some(t) if file.len > t.len => plan.embed.push((file.clone(), t.messages.saturating_sub(1))),
            Some(_) => plan.embed.push((file.clone(), 0)),
        }
    }
    let present: std::collections::HashSet<&std::path::Path> = disk.iter().map(|f| f.path.as_path()).collect();
    plan.gone = index.transcripts.iter().filter(|t| !present.contains(t.path.as_path())).map(|t| t.path.clone()).collect();
    plan
}

/// The chunks of a message's text worth a vector, as byte ranges: windows of at most `CHUNK_BYTES`, cut at a space in
/// their second half when there is one.
pub fn chunks(text: &str) -> Vec<(usize, usize)> {
    if text.split_whitespace().take(MIN_WORDS).count() < MIN_WORDS {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = text.floor_char_boundary((start + CHUNK_BYTES).min(text.len()));
        if end < text.len() {
            if let Some(space) = text[start..end].rfind(char::is_whitespace).filter(|&at| at > CHUNK_BYTES / 2) {
                end = start + space;
            }
        }
        if end <= start {
            end = text.ceil_char_boundary(start + 1);
        }
        if text[start..end].trim().len() > 0 {
            out.push((start, end - start));
        }
        start = end;
    }
    out
}

/// The unit vector `v` as one scale and int8 values.
pub fn quantize(v: &[f32]) -> (f32, [i8; DIM]) {
    let most = v.iter().fold(0f32, |m, x| m.max(x.abs()));
    let scale = if most > 0.0 { most / 127.0 } else { 1.0 };
    let mut out = [0i8; DIM];
    for (o, x) in out.iter_mut().zip(v) {
        *o = (x / scale).round().clamp(-127.0, 127.0) as i8;
    }
    (scale, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(message: u32, x: f32) -> Row {
        let mut v = [0f32; DIM];
        v[0] = x;
        v[1] = (1.0 - x * x).max(0.0).sqrt();
        let (scale, vector) = quantize(&v);
        Row { transcript: 0, message, start: 0, len: 10, scale, vector }
    }

    fn disk(path: &str, session: &str, len: u64, modified: u64) -> OnDisk {
        OnDisk { path: path.into(), session: session.into(), len, modified }
    }

    fn sample() -> Index {
        let mut index = Index::empty(Header::current(7));
        index.update(&disk("/a.jsonl", "a", 100, 1), 0, 2, vec![row(0, 1.0), row(1, 0.5)]);
        index.update(&disk("/b/subagents/x.jsonl", "b", 50, 2), 0, 1, vec![row(0, 0.0)]);
        index
    }

    #[test]
    fn an_index_reads_back_as_it_was_written() {
        let index = sample();
        assert_eq!(Index::decode(&index.encode(), index.header), index);
        assert_eq!(index.sessions(), 2);
    }

    #[test]
    fn an_index_of_another_format_model_chunk_or_width_reads_as_empty() {
        let index = sample();
        let bytes = index.encode();
        for other in [
            Header { format: FORMAT + 1, ..index.header },
            Header { model: 8, ..index.header },
            Header { chunk: 1000, ..index.header },
            Header { dim: 768, ..index.header },
        ] {
            assert_eq!(Index::decode(&bytes, other), Index::empty(other), "{other:?}");
        }
        assert_eq!(Index::decode(&bytes[..bytes.len() - 1], index.header), Index::empty(index.header), "cut short");
        assert_eq!(Index::decode(b"not an index", index.header), Index::empty(index.header));
    }

    #[test]
    fn new_grown_rewritten_and_gone_transcripts_each_get_their_rule() {
        let index = sample();
        let plan = plan(
            &index,
            &[
                disk("/a.jsonl", "a", 160, 3),            // grew: from its last message
                disk("/b/subagents/x.jsonl", "b", 50, 2), // unchanged
                disk("/c.jsonl", "c", 10, 1),             // new
            ],
        );
        assert_eq!(plan.embed, vec![(disk("/a.jsonl", "a", 160, 3), 1), (disk("/c.jsonl", "c", 10, 1), 0)]);
        assert!(plan.gone.is_empty());

        let plan = super::plan(&index, &[disk("/a.jsonl", "a", 90, 3)]);
        assert_eq!(plan.embed, vec![(disk("/a.jsonl", "a", 90, 3), 0)], "shorter: whole again");
        assert_eq!(plan.gone, vec![PathBuf::from("/b/subagents/x.jsonl")], "gone");

        let plan = super::plan(&index, &[disk("/a.jsonl", "a", 100, 9), disk("/b/subagents/x.jsonl", "b", 50, 2)]);
        assert_eq!(plan.embed, vec![(disk("/a.jsonl", "a", 100, 9), 0)], "same size, written again: whole again");
    }

    #[test]
    fn an_update_replaces_rows_from_its_message_on_and_a_drop_removes_them() {
        let mut index = sample();
        index.update(&disk("/a.jsonl", "a", 160, 3), 1, 3, vec![row(1, 0.2), row(2, 0.3)]);
        let a: Vec<u32> = index.rows.iter().filter(|r| r.transcript == 0).map(|r| r.message).collect();
        assert_eq!(a, [0, 1, 2]);
        assert_eq!(index.transcripts[0].messages, 3);
        index.drop_transcript(std::path::Path::new("/a.jsonl"));
        assert_eq!(index.transcripts.len(), 1);
        assert!(index.rows.iter().all(|r| r.transcript == 0), "the subagent's row now points at its new place");
        assert_eq!(index.transcripts[0].session, "b");
    }

    #[test]
    fn rank_puts_each_session_once_by_its_best_chunk() {
        let index = sample();
        let mut q = [0f32; DIM];
        q[0] = 1.0;
        let ranked = index.rank(&q);
        assert_eq!(ranked.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(ranked[0].1.message, 0);
        assert!(ranked[0].1.score > 0.98 - LONG_SESSION * 3f32.ln());
    }

    #[test]
    fn chunks_cut_long_messages_at_spaces_and_skip_short_ones() {
        assert!(chunks("yes do it").len() == 1 && chunks("ok").is_empty() && chunks("commit it").is_empty());
        let long = "word ".repeat(700);
        let cut = chunks(&long);
        assert!(cut.len() >= 3);
        assert!(cut.iter().all(|&(_, len)| len <= CHUNK_BYTES));
        assert_eq!(cut.iter().map(|&(_, len)| len).sum::<usize>(), long.len(), "nothing lost between chunks");
        let wide = "čšž ".repeat(600);
        assert!(chunks(&wide).iter().all(|&(s, len)| wide.is_char_boundary(s) && wide.is_char_boundary(s + len)));
    }
}
