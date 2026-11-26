use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    Chrome,
    Firefox,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePackage {
    pub store: Store,
    /// The 32 letter Chrome extension id, or the AMO slug.
    pub id: String,
    /// Chrome: the CRX itself (the update service answers with a redirect to it). Firefox: the AMO API URL whose JSON
    /// [`amo_file_url`] turns into the `.xpi` URL.
    pub download: String,
}

/// A Chrome version new enough for the Web Store: it answers 204 (no file) to Chrome older than 88, and this value is
/// what crxviewer falls back to (`cws_pattern.js`). The update service does not check it against a real build.
const PRODVERSION: &str = "9999.0.9999.0";

/// What the store page at `page_url` can be downloaded as, for `https` pages of the Chrome Web Store
/// (`chromewebstore.google.com/detail/<slug>/<id>` and the old `chrome.google.com/webstore/detail/<slug>/<id>`) and
/// addons.mozilla.org (`/<locale>/firefox/addon/<slug>/`). Anything else is `None`.
pub fn store_package(page_url: &str) -> Option<StorePackage> {
    let url = Url::parse(page_url).ok()?;
    if url.scheme() != "https" || url.port().is_some() || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let segments: Vec<&str> = url.path_segments()?.collect();
    // A trailing slash leaves an empty last segment.
    let segments: Vec<&str> = match segments.split_last() {
        Some((&"", rest)) => rest.to_vec(),
        _ => segments,
    };
    match url.host_str()? {
        "chromewebstore.google.com" => chrome(&segments),
        "chrome.google.com" => match segments.split_first() {
            Some((&"webstore", rest)) => chrome(rest),
            _ => None,
        },
        "addons.mozilla.org" => firefox(&segments),
        _ => None,
    }
}

/// `detail/<slug>/<id>` or `detail/<id>`, either with a tab such as `reviews` after it.
fn chrome(segments: &[&str]) -> Option<StorePackage> {
    let ["detail", rest @ ..] = segments else { return None };
    // Chromium's `IdIsValid`: 32 characters, each a to p.
    let id = *rest.iter().take(2).find(|s| s.len() == 32 && s.bytes().all(|b| (b'a'..=b'p').contains(&b)))?;
    Some(StorePackage {
        store: Store::Chrome,
        id: id.to_string(),
        download: format!("https://clients2.google.com/service/update2/crx?response=redirect&prodversion={PRODVERSION}&acceptformat=crx2,crx3&x=id%3D{id}%26uc"),
    })
}

/// `<locale>/firefox/addon/<slug>` or `firefox/addon/<slug>`.
fn firefox(segments: &[&str]) -> Option<StorePackage> {
    let slug = match segments {
        [_, "firefox", "addon", slug] => *slug,
        ["firefox", "addon", slug] => *slug,
        _ => return None,
    };
    let lower = slug.to_ascii_lowercase();
    // The slug goes into a URL path as written, so it must stay one segment.
    if slug.is_empty() || slug == "." || slug == ".." || lower.contains("%2f") || lower.contains("%5c") || lower.contains("%2e%2e") {
        return None;
    }
    Some(StorePackage { store: Store::Firefox, id: slug.to_string(), download: format!("https://addons.mozilla.org/api/v5/addons/addon/{slug}/") })
}

/// The `.xpi` URL in an AMO add-on API response (`current_version.file.url`), if it is an `https` addons.mozilla.org URL.
pub fn amo_file_url(api_json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(api_json).ok()?;
    let file = v.pointer("/current_version/file/url")?.as_str()?;
    let url = Url::parse(file).ok()?;
    (url.scheme() == "https" && url.host_str() == Some("addons.mozilla.org") && url.port().is_none() && url.username().is_empty()).then(|| url.to_string())
}
