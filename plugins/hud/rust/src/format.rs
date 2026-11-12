use serde_json::Value;

pub const MINUTE: f64 = 60_000.0;

/// A value as one line of text, at most `max` characters: what JavaScript's `String` makes of it, with every run of
/// white space one space.
pub fn text(value: &Value, max: usize) -> String {
    let raw = match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// `6.6 GB`, `16 GB`, rounded as claude-hud does.
pub fn gb(bytes: f64) -> String {
    const GB: f64 = 1_073_741_824.0;
    if bytes >= 10.0 * GB { format!("{:.0} GB", bytes / GB) } else { format!("{:.1} GB", bytes / GB) }
}

/// The first `max` names, then how many more.
pub fn names(all: &[String], max: usize) -> String {
    if all.len() > max { format!("{}, +{} more", all[..max].join(", "), all.len() - max) } else { all.join(", ") }
}

/// `2h 54m`, `2d 11h`, `5m`; `compact` drops the spaces and a zero remainder for the strip: `2h54m`, `2d11h`, `3d`.
pub fn until(epoch_seconds: f64, now: f64, compact: bool) -> String {
    let minutes = ((epoch_seconds * 1000.0 - now) / MINUTE).round().max(0.0) as u64;
    let (days, hours, minutes) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    let gap = if compact { "" } else { " " };
    match (days, hours) {
        (0, 0) => format!("{minutes}m"),
        (0, _) => format!("{hours}h{gap}{minutes}m"),
        (_, 0) if compact => format!("{days}d"),
        _ => format!("{days}d{gap}{hours}h"),
    }
}
