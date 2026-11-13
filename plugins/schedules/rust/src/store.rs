//! The saved schedules and "assigned" entries, under the keys Schedules has always used. Main is the only writer.

use crate::{
    assigned::Watch,
    schedule::Schedule,
};
use serde_json::Value;
use wings_plugin::host::storage;

pub const SCHEDULES: &str = "schedules";
pub const WATCHES: &str = "assigned";

pub fn read_schedules() -> Result<Vec<Schedule>, String> {
    Ok(storage::get(SCHEDULES)?.as_array().map(|list| list.iter().filter_map(Schedule::from_json).collect()).unwrap_or_default())
}

pub fn read_watches() -> Result<Vec<Watch>, String> {
    Ok(storage::get(WATCHES)?.as_array().map(|list| list.iter().filter_map(Watch::from_json).collect()).unwrap_or_default())
}

pub fn save_schedules(list: &[Schedule]) -> Result<(), String> {
    storage::set(SCHEDULES, &Value::Array(list.iter().map(Schedule::to_json).collect()))
}

pub fn save_watches(list: &[Watch]) -> Result<(), String> {
    storage::set(WATCHES, &Value::Array(list.iter().map(Watch::to_json).collect()))
}
