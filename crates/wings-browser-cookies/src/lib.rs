//! Reads the cookies of the other browsers installed on this Mac (Chromium family, Firefox, Safari), so the
//! built-in Browser can import them when the user asks.
//!
//! Nothing here touches the network, and nothing is read until `read`: `detect` only looks at which cookie stores
//! exist, so the Keychain prompt appears on the user's click and not before.

mod chromium;
mod crypto;
mod firefox;
mod safari;
#[cfg(target_os = "macos")]
mod system;

use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Browser {
    Chrome,
    Arc,
    Brave,
    Edge,
    Chromium,
    Vivaldi,
    Firefox,
    Safari,
}

impl Browser {
    pub fn name(self) -> &'static str {
        match self {
            Browser::Chrome => "Google Chrome",
            Browser::Arc => "Arc",
            Browser::Brave => "Brave",
            Browser::Edge => "Microsoft Edge",
            Browser::Chromium => "Chromium",
            Browser::Vivaldi => "Vivaldi",
            Browser::Firefox => "Firefox",
            Browser::Safari => "Safari",
        }
    }

    /// The lowercase prefix of a source's id.
    pub fn slug(self) -> &'static str {
        match self {
            Browser::Chrome => "chrome",
            Browser::Arc => "arc",
            Browser::Brave => "brave",
            Browser::Edge => "edge",
            Browser::Chromium => "chromium",
            Browser::Vivaldi => "vivaldi",
            Browser::Firefox => "firefox",
            Browser::Safari => "safari",
        }
    }
}

/// One cookie store the user can import from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// Stable across runs, for example `chrome:Default` or `firefox:Profiles/abc.default-release`.
    pub id: String,
    pub browser: Browser,
    pub browser_name: String,
    /// The profile's display name in the browser itself.
    pub profile: String,
    /// The cookie store: a `Cookies` or `cookies.sqlite` file, or Safari's `Cookies.binarycookies`.
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SameSite {
    None,
    Lax,
    Strict,
}

#[derive(Clone, PartialEq)]
pub struct Cookie {
    /// As stored: a leading dot marks a cookie for the domain and its subdomains.
    pub domain: String,
    pub name: String,
    pub value: String,
    pub path: String,
    /// Seconds since the Unix epoch; `None` for a session cookie.
    pub expires: Option<f64>,
    pub secure: bool,
    pub http_only: bool,
    /// `None` when the browser did not store one.
    pub same_site: Option<SameSite>,
}

// The value is a credential: it never reaches a log through `{:?}`.
impl fmt::Debug for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cookie")
            .field("domain", &self.domain)
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("path", &self.path)
            .field("expires", &self.expires)
            .finish_non_exhaustive()
    }
}

/// What one import read.
#[derive(Debug, Default, PartialEq)]
pub struct Read {
    pub cookies: Vec<Cookie>,
    /// Cookies left out because they could not be decrypted or decoded. Expired ones are not counted.
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The user refused, or cancelled, the Keychain prompt for the browser's "Safe Storage" key.
    KeychainDenied,
    /// macOS blocks the read until Wings has Full Disk Access.
    NeedsFullDiskAccess,
    /// The cookie store is gone, or this is not a Mac.
    NotFound,
    Unreadable(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::KeychainDenied => f.write_str(
                "Keychain access was denied. Allow Wings to use the browser's \"Safe Storage\" key to import its cookies.",
            ),
            ReadError::NeedsFullDiskAccess => f.write_str(
                "macOS is blocking access to this browser's data. Turn on Full Disk Access for Wings in System Settings > Privacy & Security, then import again.",
            ),
            ReadError::NotFound => f.write_str("This browser's cookies were not found."),
            ReadError::Unreadable(why) => write!(f, "Could not read this browser's cookies: {why}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// The cookie stores found on this Mac, without reading any cookie and without touching the Keychain.
#[cfg(target_os = "macos")]
pub fn detect() -> Vec<Source> {
    system::detect()
}

/// The cookies of one store: expired ones are dropped, undecryptable ones counted. A Chromium browser's encrypted
/// values make macOS ask the user to allow the Keychain read, so call this from the user's click.
#[cfg(target_os = "macos")]
pub fn read(source: &Source) -> Result<Read, ReadError> {
    system::read(source)
}

#[cfg(not(target_os = "macos"))]
pub fn detect() -> Vec<Source> {
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
pub fn read(_source: &Source) -> Result<Read, ReadError> {
    Err(ReadError::NotFound)
}

/// Seconds since the Unix epoch now.
pub(crate) fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}
