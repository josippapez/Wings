//! What a web view asks the person before it goes on, whichever engine asks: a site whose certificate isn't valid, a
//! server that wants a name and a password, the page's zoom and its find bar. The decisions are plain functions here so
//! both engines answer alike and tests can pin them; the engines (`chromium/page.rs`, `mac.rs`) only carry them out.
//!
//! Nothing here is told to a plugin that the engine keeps for itself: a password goes from Wings' sheet to the engine and
//! no further.

use std::{cell::RefCell, collections::HashSet, path::Path};

/// The site a page was asked to open, as a certificate choice names it: scheme-less `host:port` in lower case, with no
/// user name or password, which `https://user:pw@Host:8443/x` reads as `host:8443`. None for an address with no host.
pub fn site_of(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    (!authority.is_empty() && !authority.chars().any(char::is_control)).then(|| authority.to_ascii_lowercase())
}

/// The host of a `site_of` name, with no port (`[::1]:8443` is `[::1]`), for words the person reads.
pub fn host_of(site: &str) -> &str {
    match site.rfind(':') {
        Some(at) if !site[at + 1..].is_empty() && site[at + 1..].bytes().all(|byte| byte.is_ascii_digit()) => &site[..at],
        _ => site,
    }
}

/// A certificate as a choice about it remembers it: the SHA-256 of its DER bytes.
pub type Fingerprint = [u8; 32];

/// The fingerprint of a certificate's DER bytes.
pub fn fingerprint(der: &[u8]) -> Fingerprint {
    use sha2::Digest;
    sha2::Sha256::digest(der).into()
}

thread_local! {
    /// The certificates the person chose to open a site with despite their errors, by the plugin's web folder and the site:
    /// kept for this run only, never written down, and for one profile only, as its cookies are. Another certificate for the
    /// same site asks again, as Chrome's choice is for a host and a certificate.
    static TRUSTED: RefCell<HashSet<(std::path::PathBuf, String, Fingerprint)>> = RefCell::default();
}

/// Whether the person chose to open `site` (`site_of`) with the certificate `cert` despite its errors, in the pages of the
/// web folder `profile`. A certificate Chromium didn't hand over (`None`) is never trusted.
pub fn trusted(profile: &Path, site: &str, cert: Option<Fingerprint>) -> bool {
    cert.is_some_and(|cert| TRUSTED.with_borrow(|trusted| trusted.contains(&(profile.to_path_buf(), site.to_owned(), cert))))
}

/// Notes that the person chose to open `site` with the certificate `cert` despite its errors, for the rest of this run.
pub fn trust(profile: &Path, site: &str, cert: Option<Fingerprint>) {
    let Some(cert) = cert else { return };
    TRUSTED.with_borrow_mut(|trusted| {
        trusted.insert((profile.to_path_buf(), site.to_owned(), cert));
    });
}

/// Forgets every choice, as a test starts afresh.
#[cfg(test)]
pub fn forget_all() {
    TRUSTED.with_borrow_mut(HashSet::clear);
}

/// What a certificate error says to the person, from Chromium's error name (`net::ERR_CERT_AUTHORITY_INVALID`): the
/// reason in words, which is all the notice shows of it.
pub fn chromium_reason(name: &str) -> &'static str {
    match name.trim_start_matches("net::") {
        "ERR_CERT_AUTHORITY_INVALID" => "The certificate isn't from an authority this Mac trusts: it may be self-signed.",
        "ERR_CERT_COMMON_NAME_INVALID" => "The certificate is for another site's name.",
        "ERR_CERT_DATE_INVALID" => "The certificate has expired or isn't valid yet.",
        "ERR_CERT_REVOKED" => "The certificate was revoked.",
        "ERR_CERT_WEAK_SIGNATURE_ALGORITHM" | "ERR_CERT_WEAK_KEY" => "The certificate uses a weak key or signature.",
        "ERR_CERT_NAME_CONSTRAINT_VIOLATION" | "ERR_CERT_INVALID" => "The certificate isn't valid.",
        "ERR_CERT_NO_REVOCATION_MECHANISM" | "ERR_CERT_UNABLE_TO_CHECK_REVOCATION" => "The certificate's revocation couldn't be checked.",
        "ERR_CERT_CONTAINS_ERRORS" => "The certificate has errors.",
        _ => "The site's certificate isn't valid.",
    }
}

/// The Foundation error codes (`NSURLErrorDomain`) WebKit fails a load with for a certificate it doesn't accept: has a bad
/// date, is untrusted, has an unknown root, isn't valid yet.
pub const WEBKIT_CERTIFICATE_ERRORS: [isize; 4] = [-1201, -1202, -1203, -1204];

/// What a certificate error says to the person, from the `NSURLError` code WebKit failed the load with, or none when the
/// code is no certificate error.
pub fn webkit_reason(code: isize) -> Option<&'static str> {
    Some(match code {
        -1201 => "The certificate has expired or isn't valid yet.",
        -1202 => "The certificate isn't from an authority this Mac trusts: it may be self-signed.",
        -1203 => "The certificate was signed by an unknown authority.",
        -1204 => "The certificate isn't valid yet.",
        _ => return None,
    })
}

/// A certificate notice, which the page's plugin draws with its own two buttons.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notice {
    /// `host:port` as the person reads it.
    pub site: String,
    pub reason: String,
    /// The certificate the site showed, which a proceed trusts: never told to a plugin.
    pub cert: Option<Fingerprint>,
}

/// The notice for a load of `url` that failed its certificate `cert` for `reason`, or none for a site the person already
/// chose to open with that certificate (the load goes on without asking) or an address with no site.
pub fn notice(profile: &Path, url: &str, reason: &str, cert: Option<Fingerprint>) -> Option<Notice> {
    let site = site_of(url)?;
    (!trusted(profile, &site, cert)).then(|| Notice { site, reason: reason.to_owned(), cert })
}

/// What the person chose about a certificate notice: "Proceed to <site> (unsafe)" or "Go back".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Proceed,
    Back,
}

/// Carries out `choice` for the notice of `waiting`: a proceed remembers the site for the run and says so, a "go back"
/// remembers nothing. None when no notice is waiting, so a stray command does nothing.
pub fn decide(profile: &Path, waiting: Option<&Notice>, choice: Choice) -> Option<Choice> {
    let waiting = waiting?;
    if choice == Choice::Proceed {
        trust(profile, &waiting.site, waiting.cert);
    }
    Some(choice)
}

/// The server a sign-in sheet names: its host, with the port unless it is the scheme's usual 80 or 443.
pub fn auth_site(host: &str, port: i32) -> String {
    match port {
        80 | 443 | 0 => host.to_owned(),
        port => format!("{host}:{port}"),
    }
}

/// What the sheet that asks for a name and a password says: its title (the site) and its message, from the server's
/// `realm` (may be empty), whether a proxy asks (`proxy`) and whether an earlier try failed.
pub fn auth_words(site: &str, realm: &str, proxy: bool, failed_before: bool) -> (String, String) {
    let title = if proxy { format!("The proxy {site} needs a name and a password") } else { format!("Sign in to {site}") };
    let realm: String = realm.chars().filter(|c| !c.is_control()).take(200).collect();
    let mut message = match realm.is_empty() {
        true => "Enter the name and password this site asks for.".to_owned(),
        false => format!("Enter the name and password for \"{realm}\"."),
    };
    if failed_before {
        message.push_str(" The last ones weren't accepted.");
    }
    (title, message)
}

/// Whether a server's challenge is one Wings asks the person for: basic and digest, over `https` or a plain `http` the
/// person typed, never NTLM or Negotiate, which the engine's own handling keeps.
pub fn asks_for_a_password(scheme: &str) -> bool {
    matches!(scheme.to_ascii_lowercase().as_str(), "basic" | "digest")
}

/// The words the sheet's two fields end with: an empty name is no answer, which is a cancel.
pub fn credentials(name: &str, password: &str) -> Option<(String, String)> {
    (!name.is_empty()).then(|| (name.to_owned(), password.to_owned()))
}

/// What a find bar is told of a search that found `matches` (none where the engine can't count) with `current` the active
/// one: on a page signed in with imported cookies only that something was found, never how many or whether it was, so a
/// plugin can't read the page by asking whether it holds a word. The marks in the page show the person all of it.
pub fn shown_find(matches: Option<u32>, current: u32, imported: bool) -> (Option<u32>, u32, bool) {
    match imported {
        true => (None, 0, true),
        false => (matches, current, matches.is_none_or(|count| count > 0)),
    }
}

/// The zoom levels a step moves between, in percent: Chrome's own.
pub const ZOOM_LEVELS: [u32; 15] = [25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300];

/// The page zoom after one step in (`+1`) or out (`-1`) from `percent`, or the reset (`0`): the next level beyond it, kept
/// within the list.
pub fn zoom_after(percent: u32, step: i8) -> u32 {
    match step {
        0 => 100,
        s if s > 0 => ZOOM_LEVELS.iter().copied().find(|level| *level > percent).unwrap_or(ZOOM_LEVELS[ZOOM_LEVELS.len() - 1]),
        _ => ZOOM_LEVELS.iter().rev().copied().find(|level| *level < percent).unwrap_or(ZOOM_LEVELS[0]),
    }
}

/// Chromium's zoom level for a zoom of `percent`: its steps are powers of 1.2, so 100% is 0 and 120% is 1.
pub fn chromium_level(percent: u32) -> f64 {
    (f64::from(percent) / 100.0).ln() / 1.2f64.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_is_its_host_and_port_in_lower_case_without_a_name_or_password() {
        assert_eq!(site_of("https://User:pw@Local.Test:8443/a?b#c").as_deref(), Some("local.test:8443"));
        assert_eq!(site_of("http://localhost/").as_deref(), Some("localhost"));
        assert_eq!(site_of("about:blank"), None);
        assert_eq!(site_of("https:///x"), None);
        assert_eq!(host_of("local.test:8443"), "local.test");
        assert_eq!(host_of("localhost"), "localhost");
        assert_eq!(host_of("[::1]:8443"), "[::1]");
    }

    #[test]
    fn a_site_the_person_chose_to_open_is_remembered_for_that_profile_site_and_certificate_only() {
        forget_all();
        let (mine, other) = (Path::new("/web/browser"), Path::new("/web/another"));
        let (cert, another) = (Some(fingerprint(b"self-signed")), Some(fingerprint(b"another certificate")));
        let waiting = notice(mine, "https://localhost:8443/", "x", cert).expect("a site not chosen yet asks");
        assert_eq!(waiting, Notice { site: "localhost:8443".into(), reason: "x".into(), cert });
        assert_eq!(decide(mine, Some(&waiting), Choice::Back), Some(Choice::Back));
        assert!(notice(mine, "https://localhost:8443/", "x", cert).is_some(), "going back remembers nothing");
        assert_eq!(decide(mine, Some(&waiting), Choice::Proceed), Some(Choice::Proceed));
        assert!(notice(mine, "https://localhost:8443/a", "x", cert).is_none(), "the same site and certificate go on without asking");
        assert!(notice(mine, "https://localhost:8443/", "x", another).is_some(), "another certificate for the site asks again");
        assert!(notice(mine, "https://localhost:8443/", "x", None).is_some(), "a certificate Chromium didn't hand over asks");
        assert!(notice(mine, "https://localhost:9443/", "x", cert).is_some(), "another port is another site");
        assert!(notice(mine, "https://other.test/", "x", cert).is_some(), "another host is another site");
        assert!(notice(other, "https://localhost:8443/", "x", cert).is_some(), "another profile is asked again");
    }

    #[test]
    fn a_proceed_with_nothing_waiting_or_no_certificate_remembers_nothing() {
        forget_all();
        let mine = Path::new("/web/browser");
        assert_eq!(decide(mine, None, Choice::Proceed), None);
        let waiting = notice(mine, "https://localhost/", "x", None).unwrap();
        assert_eq!(decide(mine, Some(&waiting), Choice::Proceed), Some(Choice::Proceed));
        assert!(!trusted(mine, "localhost", None));
        assert!(notice(mine, "https://localhost/", "x", None).is_some());
    }

    #[test]
    fn certificate_errors_say_why_in_words_for_both_engines() {
        assert!(chromium_reason("net::ERR_CERT_AUTHORITY_INVALID").contains("self-signed"));
        assert!(chromium_reason("ERR_CERT_DATE_INVALID").contains("expired"));
        assert!(chromium_reason("net::ERR_CERT_COMMON_NAME_INVALID").contains("another site"));
        assert_eq!(chromium_reason("net::ERR_SOMETHING"), "The site's certificate isn't valid.");
        for code in WEBKIT_CERTIFICATE_ERRORS {
            assert!(webkit_reason(code).is_some(), "{code}");
        }
        assert_eq!(webkit_reason(-1009), None, "no connection is no certificate error");
        assert_eq!(webkit_reason(-1202).unwrap(), chromium_reason("ERR_CERT_AUTHORITY_INVALID"));
    }

    #[test]
    fn the_password_sheet_names_the_site_or_proxy_and_the_realm_and_says_when_a_try_failed() {
        let (title, message) = auth_words("intranet.test", "Staff area", false, false);
        assert_eq!((title.as_str(), message.as_str()), ("Sign in to intranet.test", "Enter the name and password for \"Staff area\"."));
        let (title, message) = auth_words("proxy.test:3128", "", true, true);
        assert_eq!(title, "The proxy proxy.test:3128 needs a name and a password");
        assert_eq!(message, "Enter the name and password this site asks for. The last ones weren't accepted.");
        assert!(!auth_words("a", "line\nbreak", false, false).1.contains('\n'), "a realm is one line");
        assert!(auth_words("a", &"x".repeat(1000), false, false).1.len() < 300, "and short");
    }

    #[test]
    fn a_sign_in_names_the_server_with_its_port_unless_it_is_the_usual_one() {
        assert_eq!(auth_site("intranet.test", 443), "intranet.test");
        assert_eq!(auth_site("localhost", 8080), "localhost:8080");
    }

    #[test]
    fn only_basic_and_digest_are_asked_for_and_an_empty_name_is_a_cancel() {
        assert!(asks_for_a_password("Basic") && asks_for_a_password("digest"));
        assert!(!asks_for_a_password("ntlm") && !asks_for_a_password("negotiate") && !asks_for_a_password(""));
        assert_eq!(credentials("ana", "pw"), Some(("ana".into(), "pw".into())));
        assert_eq!(credentials("ana", ""), Some(("ana".into(), String::new())));
        assert_eq!(credentials("", "pw"), None);
    }

    #[test]
    fn a_find_on_a_page_signed_in_with_imported_cookies_tells_nothing_of_what_it_holds() {
        assert_eq!(shown_find(Some(3), 2, false), (Some(3), 2, true));
        assert_eq!(shown_find(Some(0), 0, false), (Some(0), 0, false), "no match is said on an ordinary page");
        assert_eq!(shown_find(None, 0, false), (None, 0, true), "an engine that can't count says only that it was asked");
        assert_eq!(shown_find(Some(0), 0, true), (None, 0, true), "a page with nothing found tells the same as one with matches");
        assert_eq!(shown_find(Some(9), 4, true), (None, 0, true));
    }

    #[test]
    fn zoom_steps_through_chromes_levels_and_resets() {
        assert_eq!(zoom_after(100, 1), 110);
        assert_eq!(zoom_after(110, 1), 125);
        assert_eq!(zoom_after(100, -1), 90);
        assert_eq!(zoom_after(25, -1), 25, "never below the least");
        assert_eq!(zoom_after(300, 1), 300, "never above the most");
        assert_eq!(zoom_after(175, 0), 100);
        assert_eq!(zoom_after(105, 1), 110, "from between levels it goes to the next");
        assert!(chromium_level(100).abs() < 1e-9);
        assert!((chromium_level(120) - 1.0).abs() < 1e-9);
        assert!(chromium_level(50) < 0.0);
    }
}
