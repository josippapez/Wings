use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;
use std::io::{self, Read, Write};

pub const VERSION: u8 = 1;
pub const PLUGIN_ID: &str = "tasks";
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ROWS: usize = 2000;
pub const MAX_TITLE_UTF16: usize = 200;
pub const MAX_ROW_ID_UTF16: usize = 20;
pub const MAX_ERROR_UTF16: usize = 500;
const MAX_PROJECT_ID_BYTES: usize = 4096;
const MAX_PROJECT_NAME_BYTES: usize = 1024;
const MAX_STORAGE_ENTRIES: usize = 16384;
const MAX_KEY_BYTES: usize = 8192;

/// The host selects the installation and project namespace before constructing this snapshot.
pub type StorageSnapshot = Vec<(String, Value)>;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    Invalid(&'static str),
    JavaScript,
    Deadline,
    Worker,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Plugin I/O failed: {error}"),
            Self::Json(_) => f.write_str("Invalid plugin JSON"),
            Self::Invalid(message) => f.write_str(message),
            Self::JavaScript => f.write_str("Plugin JavaScript failed"),
            Self::Deadline => f.write_str("Plugin deadline exceeded"),
            Self::Worker => f.write_str("Plugin worker failed"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Refresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub plugin_id: String,
    pub project: Project,
    pub storage: StorageSnapshot,
    #[serde(deserialize_with = "Option::<Action>::deserialize")]
    pub action: Option<Action>,
}

impl Request {
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != VERSION || self.plugin_id != PLUGIN_ID {
            return Err(Error::Invalid("Unsupported plugin identity or version"));
        }
        input_text(&self.project.id, MAX_PROJECT_ID_BYTES)?;
        input_text(&self.project.name, MAX_PROJECT_NAME_BYTES)?;
        if self.storage.len() > MAX_STORAGE_ENTRIES {
            return Err(Error::Invalid("Too many storage entries"));
        }
        let mut keys = HashSet::new();
        for (key, _) in &self.storage {
            input_text(key, MAX_KEY_BYTES)?;
            if !keys.insert(key) {
                return Err(Error::Invalid("Duplicate storage key"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenState {
    Ready,
    Empty,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Todo,
    InProgress,
    InReview,
    Done,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    Task,
    Bug,
    Feature,
    Epic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub status: Status,
    #[serde(rename = "type")]
    pub task_type: TaskType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenAction {
    pub id: Action,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub version: u8,
    pub title: String,
    pub state: ScreenState,
    pub rows: Vec<Row>,
    pub actions: Vec<ScreenAction>,
    #[serde(
        default,
        deserialize_with = "optional_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<String>,
}

fn optional_error<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

impl Screen {
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != VERSION {
            return Err(Error::Invalid("Unsupported screen version"));
        }
        text(&self.title, MAX_TITLE_UTF16)?;
        if self.rows.len() > MAX_ROWS {
            return Err(Error::Invalid("Too many screen rows"));
        }
        let mut ids = HashSet::new();
        for row in &self.rows {
            text(&row.id, MAX_ROW_ID_UTF16)?;
            row_title(&row.title)?;
            if !ids.insert(&row.id) {
                return Err(Error::Invalid("Duplicate screen row id"));
            }
        }
        let mut actions = HashSet::new();
        for action in &self.actions {
            text(&action.label, MAX_TITLE_UTF16)?;
            if !actions.insert(action.id) {
                return Err(Error::Invalid("Duplicate screen action id"));
            }
        }
        if self.actions.len() != 1 {
            return Err(Error::Invalid(
                "Screen must offer exactly one Refresh action",
            ));
        }
        if let Some(error) = &self.error {
            text(error, MAX_ERROR_UTF16)?;
        } else if self.state == ScreenState::Error {
            return Err(Error::Invalid("Error screen must include an error"));
        }
        Ok(())
    }
}

fn row_title(value: &str) -> Result<(), Error> {
    // Issues uses JavaScript trim(), which trims BOM but treats NEL as content.
    let trimmed =
        value.trim_matches(|c: char| c == '\u{feff}' || (c.is_whitespace() && c != '\u{0085}'));
    if trimmed.is_empty() || value.encode_utf16().count() > MAX_TITLE_UTF16 {
        return Err(Error::Invalid("Invalid issue title"));
    }
    Ok(())
}

fn text(value: &str, max_utf16: usize) -> Result<(), Error> {
    if value.trim().is_empty()
        || value.encode_utf16().count() > max_utf16
        || value.chars().any(char::is_control)
    {
        return Err(Error::Invalid("Invalid plugin display string"));
    }
    Ok(())
}

fn input_text(value: &str, max_bytes: usize) -> Result<(), Error> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(Error::Invalid("Invalid plugin input string"));
    }
    Ok(())
}

/// Exactly one big-endian u32 length prefix, JSON payload, and EOF.
pub fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, Error> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(Error::Invalid("Invalid plugin frame length"));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    let mut trailing = [0];
    loop {
        match reader.read(&mut trailing) {
            Ok(0) => return Ok(payload),
            Ok(_) => return Err(Error::Invalid("Trailing plugin input")),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> Result<(), Error> {
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(Error::Invalid("Invalid plugin frame length"));
    }
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(payload)?;
    writer.flush()?;
    Ok(())
}

/// Serialize without allowing a Rust-side JSON buffer to grow past the frame limit.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > MAX_FRAME_BYTES - self.0.len() {
                return Err(io::Error::other("Plugin frame exceeds limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded(Vec::new());
    serde_json::to_writer(&mut output, value)?;
    Ok(output.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn screen_json() -> Value {
        json!({"version":1,"title":"Issues","state":"ready","rows":[
            {"id":"WIN-1","title":"First","status":"todo","type":"task"}
        ],"actions":[{"id":"refresh","label":"Refresh"}]})
    }

    #[test]
    fn frame_round_trip_and_bad_lengths() {
        let mut frame = Vec::new();
        write_frame(&mut frame, b"{}").unwrap();
        assert_eq!(read_frame(&mut frame.as_slice()).unwrap(), b"{}");
        for bytes in [
            vec![],
            vec![0, 0],
            vec![0; 4],
            u32::MAX.to_be_bytes().to_vec(),
            vec![0, 0, 0, 3, b'{'],
        ] {
            assert!(read_frame(&mut bytes.as_slice()).is_err());
        }
        frame.push(0);
        assert!(read_frame(&mut frame.as_slice()).is_err());
        assert!(write_frame(&mut Vec::new(), &vec![0; MAX_FRAME_BYTES + 1]).is_err());
        assert!(encode(&"x".repeat(MAX_FRAME_BYTES)).is_err());
    }

    #[test]
    fn maximum_frame_length_is_inclusive() {
        let payload = vec![b'x'; MAX_FRAME_BYTES];
        let mut frame = Vec::new();
        write_frame(&mut frame, &payload).unwrap();
        assert_eq!(read_frame(&mut frame.as_slice()).unwrap(), payload);
        assert_eq!(
            encode(&"x".repeat(MAX_FRAME_BYTES - 2)).unwrap().len(),
            MAX_FRAME_BYTES
        );
    }

    #[test]
    fn oversized_header_is_rejected_before_reading_payload() {
        struct HeaderOnly(Option<[u8; 4]>);
        impl Read for HeaderOnly {
            fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
                let header = self.0.take().expect("must not read oversize payload");
                target.copy_from_slice(&header);
                Ok(4)
            }
        }
        assert!(
            read_frame(&mut HeaderOnly(Some(
                ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes()
            )))
            .is_err()
        );
    }

    #[test]
    fn screen_rejects_unknown_fields_variants_and_wrong_types() {
        let original = screen_json();
        let mut bad = Vec::new();
        for (pointer, value) in [
            ("/version", json!(2)),
            ("/state", json!("loading")),
            ("/rows/0/status", json!("running")),
            ("/rows/0/type", json!("story")),
            ("/actions/0/id", json!("delete")),
            ("/title", json!(2)),
        ] {
            let mut value_json = original.clone();
            *value_json.pointer_mut(pointer).unwrap() = value;
            bad.push(value_json);
        }
        for pointer in ["", "/rows/0", "/actions/0"] {
            let mut value = original.clone();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unknown".into(), json!(true));
            bad.push(value);
        }
        let mut null_error = original.clone();
        null_error["error"] = Value::Null;
        bad.push(null_error);
        for value in bad {
            assert!(
                serde_json::from_value::<Screen>(value)
                    .and_then(|screen| screen.validate().map_err(serde::de::Error::custom))
                    .is_err()
            );
        }
    }

    #[test]
    fn screen_bounds_and_duplicate_ids() {
        let valid: Screen = serde_json::from_value(screen_json()).unwrap();
        valid.validate().unwrap();
        let mut variants = Vec::new();
        for title in [
            "".into(),
            " ".into(),
            "x\0y".into(),
            "x".repeat(MAX_TITLE_UTF16 + 1),
        ] {
            let mut screen = valid.clone();
            screen.title = title;
            variants.push(screen);
        }
        let mut screen = valid.clone();
        screen.rows.push(screen.rows[0].clone());
        variants.push(screen);
        let mut screen = valid.clone();
        screen.actions.push(screen.actions[0].clone());
        variants.push(screen);
        let mut screen = valid.clone();
        screen.rows = (0..=MAX_ROWS)
            .map(|i| Row {
                id: i.to_string(),
                ..valid.rows[0].clone()
            })
            .collect();
        variants.push(screen);
        let mut screen = valid.clone();
        screen.rows[0].id = "x".repeat(MAX_ROW_ID_UTF16 + 1);
        variants.push(screen);
        for title in [
            "".into(),
            " \t\n".into(),
            "\u{feff}".into(),
            "x".repeat(MAX_TITLE_UTF16 + 1),
        ] {
            let mut screen = valid.clone();
            screen.rows[0].title = title;
            variants.push(screen);
        }
        let mut screen = valid.clone();
        screen.rows[0].id = "x\ny".into();
        variants.push(screen);
        let mut screen = valid.clone();
        screen.actions[0].label = "".into();
        variants.push(screen);
        let mut screen = valid.clone();
        screen.actions[0].label = "x\ty".into();
        variants.push(screen);
        let mut screen = valid.clone();
        screen.error = Some("x\ny".into());
        variants.push(screen);
        let mut screen = valid;
        screen.error = Some("x".repeat(MAX_ERROR_UTF16 + 1));
        variants.push(screen);
        for screen in variants {
            assert!(screen.validate().is_err());
        }
    }

    #[test]
    fn screen_limits_match_typescript_utf16_units() {
        let mut screen: Screen = serde_json::from_value(screen_json()).unwrap();
        screen.title = "😀".repeat(100);
        screen.rows[0].title = "界".repeat(200);
        screen.rows[0].id = "😀".repeat(10);
        screen.actions[0].label = "😀".repeat(100);
        screen.state = ScreenState::Error;
        screen.error = Some("😀".repeat(250));
        screen.validate().unwrap();
        screen.title.push('x');
        assert!(screen.validate().is_err());
        screen.title.pop();
        screen.rows[0].id.push('x');
        assert!(screen.validate().is_err());
        screen.rows[0].id.pop();
        screen.rows[0].title.push('x');
        assert!(screen.validate().is_err());
        screen.rows[0].title.pop();
        screen.actions[0].label.push('x');
        assert!(screen.validate().is_err());
        screen.actions[0].label.pop();
        screen.error.as_mut().unwrap().push('x');
        assert!(screen.validate().is_err());
    }

    #[test]
    fn refresh_and_error_contract_fields_are_required() {
        let mut screen: Screen = serde_json::from_value(screen_json()).unwrap();
        screen.actions.clear();
        assert!(screen.validate().is_err());
        screen.actions.push(ScreenAction {
            id: Action::Refresh,
            label: "Refresh".into(),
        });
        screen.state = ScreenState::Error;
        assert!(screen.validate().is_err());
        let mut value = serde_json::to_value(Request {
            version: VERSION,
            plugin_id: PLUGIN_ID.into(),
            project: Project {
                id: "p".into(),
                name: "Project".into(),
            },
            storage: vec![],
            action: None,
        })
        .unwrap();
        value.as_object_mut().unwrap().remove("action");
        assert!(serde_json::from_value::<Request>(value).is_err());
    }

    #[test]
    fn request_binds_identity_and_readonly_snapshot() {
        let mut request = Request {
            version: VERSION,
            plugin_id: PLUGIN_ID.into(),
            project: Project {
                id: "p".into(),
                name: "Project".into(),
            },
            storage: vec![],
            action: None,
        };
        request.validate().unwrap();
        request.plugin_id = "installed-plugin".into();
        assert!(request.validate().is_err());
        request.plugin_id = PLUGIN_ID.into();
        request.version = 2;
        assert!(request.validate().is_err());
        request.version = VERSION;
        request.storage = vec![("task:1".into(), json!(null)); 2];
        assert!(request.validate().is_err());
        let mut value = serde_json::to_value(request).unwrap();
        value["source"] = json!("arbitrary JavaScript");
        assert!(serde_json::from_value::<Request>(value).is_err());
    }
}
