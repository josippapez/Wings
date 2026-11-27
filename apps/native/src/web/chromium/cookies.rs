//! Another browser's sign-ins imported into a profile that chose Chromium: each cookie goes into the request context the
//! profile's pages use (`runtime::context`), through CEF's cookie manager, by WebKit's rule (`mac::set_cookies`): kept
//! past this run only with `keep_signins`, else as a session cookie, which Chromium doesn't keep (Wings sets no
//! `persist_session_cookies`). The sites are noted as imported before any cookie goes in (`web::note_imported`).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cef::*;

use super::runtime;
use crate::web::Profile;

/// Seconds from 1601, where Chromium counts time from, to 1970.
const WINDOWS_TO_UNIX: f64 = 11_644_473_600.0;

/// A cookie as CEF's cookie manager takes it, and the address it is set for.
#[derive(Debug, PartialEq)]
pub struct Made {
    pub url: String,
    pub name: String,
    pub value: String,
    /// Empty for a cookie of its host alone; with its leading dot for one of the domain and its subdomains.
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// Microseconds since 1601 (Chromium's `base::Time`), none for a session cookie.
    pub expires: Option<i64>,
    pub same_site: SameSite,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SameSite {
    Unspecified,
    NoRestriction,
    Lax,
    Strict,
}

/// `cookie` as Chromium takes it, kept past this run only with `keep_signins`, or none for one with no host.
pub fn made(cookie: &wings_browser_cookies::Cookie, keep_signins: bool) -> Option<Made> {
    let host = cookie.domain.trim_start_matches('.');
    if host.is_empty() {
        return None;
    }
    let path = if cookie.path.is_empty() { "/".to_owned() } else { cookie.path.clone() };
    let scheme = if cookie.secure { "https" } else { "http" };
    let same_site = match cookie.same_site {
        Some(wings_browser_cookies::SameSite::Lax) => SameSite::Lax,
        Some(wings_browser_cookies::SameSite::Strict) => SameSite::Strict,
        // Chromium takes SameSite=None only on a secure cookie.
        Some(wings_browser_cookies::SameSite::None) if cookie.secure => SameSite::NoRestriction,
        _ => SameSite::Unspecified,
    };
    Some(Made {
        url: format!("{scheme}://{host}{path}"),
        name: cookie.name.clone(),
        value: cookie.value.clone(),
        domain: if cookie.domain.starts_with('.') { cookie.domain.clone() } else { String::new() },
        path,
        secure: cookie.secure,
        http_only: cookie.http_only,
        // With no expiry it is a session cookie, which goes as Wings quits, even from a kept profile.
        expires: cookie.expires.filter(|_| keep_signins).map(|seconds| ((seconds + WINDOWS_TO_UNIX) * 1_000_000.0) as i64),
        same_site,
    })
}

impl Made {
    fn cef(&self) -> Cookie {
        Cookie {
            name: CefString::from(self.name.as_str()),
            value: CefString::from(self.value.as_str()),
            domain: CefString::from(self.domain.as_str()),
            path: CefString::from(self.path.as_str()),
            secure: i32::from(self.secure),
            httponly: i32::from(self.http_only),
            has_expires: i32::from(self.expires.is_some()),
            expires: Basetime { val: self.expires.unwrap_or_default() },
            same_site: match self.same_site {
                SameSite::Unspecified => CookieSameSite::UNSPECIFIED,
                SameSite::NoRestriction => CookieSameSite::NO_RESTRICTION,
                SameSite::Lax => CookieSameSite::LAX_MODE,
                SameSite::Strict => CookieSameSite::STRICT_MODE,
            },
            ..Default::default()
        }
    }
}

type Done = Rc<RefCell<Option<Box<dyn FnOnce(Result<usize, String>)>>>>;

/// Notes the sites of `cookies` as imported, then puts them in `profile`'s Chromium store, kept or not (`keep_data`),
/// starting Chromium if it isn't running; `done` hears how many it took. A cookie Chromium refuses (a character it won't
/// take in a value) is left out, and none goes in when the sites can't be noted.
pub fn import(profile: &Profile, keep_data: bool, keep_signins: bool, cookies: &[wings_browser_cookies::Cookie], done: impl FnOnce(Result<usize, String>) + 'static) {
    if let Err(why) = crate::web::note_imported(&profile.dir, cookies.iter().map(|cookie| cookie.domain.clone())) {
        return done(Err(why));
    }
    let made: Vec<Made> = cookies.iter().filter_map(|cookie| made(cookie, keep_signins)).collect();
    // Chromium does its work while one is waiting, as while a page is open.
    runtime::opened();
    let done: Done = Rc::new(RefCell::new(Some(Box::new(move |result| {
        runtime::gone();
        done(result);
    }))));
    let profile = profile.clone();
    runtime::ensure(&profile.clone(), move |ready| {
        let finish = |result| {
            if let Some(done) = done.borrow_mut().take() {
                done(result);
            }
        };
        if let Err(why) = ready {
            return finish(Err(why));
        }
        let manager = runtime::context(&profile, keep_data).or_else(request_context_get_global_context).and_then(|context| context.cookie_manager(None));
        let Some(manager) = manager else { return finish(Err("Chromium's cookies can't be reached".into())) };
        if made.is_empty() {
            return finish(Ok(0));
        }
        let (left, took) = (Rc::new(Cell::new(made.len())), Rc::new(Cell::new(0)));
        for cookie in &made {
            let mut heard = Took::new(left.clone(), took.clone(), done.clone(), manager.clone());
            // An address CEF refuses is never answered.
            if manager.set_cookie(Some(&CefString::from(cookie.url.as_str())), Some(&cookie.cef()), Some(&mut heard)) == 0 {
                counted(&left, &took, &done, &manager, false);
            }
        }
    });
}

/// One cookie was set, or wasn't: once the last is, the store is written to disk and `done` hears how many took.
fn counted(left: &Cell<usize>, took: &Cell<usize>, done: &Done, manager: &CookieManager, success: bool) {
    took.set(took.get() + usize::from(success));
    left.set(left.get() - 1);
    if left.get() == 0 {
        manager.flush_store(None);
        if let Some(done) = done.borrow_mut().take() {
            done(Ok(took.get()));
        }
    }
}

wrap_set_cookie_callback! {
    struct Took {
        left: Rc<Cell<usize>>,
        took: Rc<Cell<usize>>,
        done: Done,
        manager: CookieManager,
    }

    impl SetCookieCallback {
        fn on_complete(&self, success: ::std::os::raw::c_int) {
            counted(&self.left, &self.took, &self.done, &self.manager, success != 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(domain: &str, secure: bool, expires: Option<f64>, same_site: Option<wings_browser_cookies::SameSite>) -> wings_browser_cookies::Cookie {
        wings_browser_cookies::Cookie {
            domain: domain.into(),
            name: "sid".into(),
            value: "v".into(),
            path: String::new(),
            expires,
            secure,
            http_only: true,
            same_site,
        }
    }

    /// A cookie of the domain keeps its leading dot, one of its host alone goes with none; its address is the one it
    /// would be sent to.
    #[test]
    fn a_cookie_keeps_whether_it_is_the_domains_or_its_hosts_alone() {
        let domain = made(&cookie(".github.com", true, None, None), false).unwrap();
        assert_eq!((domain.url.as_str(), domain.domain.as_str(), domain.path.as_str()), ("https://github.com/", ".github.com", "/"));
        let host = made(&cookie("gist.github.com", false, None, None), false).unwrap();
        assert_eq!((host.url.as_str(), host.domain.as_str()), ("http://gist.github.com/", ""));
        assert!(host.http_only && !host.secure);
        assert_eq!(made(&cookie(".", true, None, None), false), None);
    }

    /// WebKit's rule: an imported sign-in lasts past this run only when the person keeps them.
    #[test]
    fn an_imported_cookie_is_kept_past_this_run_only_with_keep_signins() {
        let expires = Some(1_800_000_000.0);
        assert_eq!(made(&cookie(".a.com", true, expires, None), false).unwrap().expires, None, "a session cookie");
        // 2027-01-15 08:00 UTC as Chromium counts it, in microseconds since 1601.
        assert_eq!(made(&cookie(".a.com", true, expires, None), true).unwrap().expires, Some(13_444_473_600_000_000));
        assert_eq!(made(&cookie(".a.com", true, None, None), true).unwrap().expires, None);
    }

    #[test]
    fn same_site_none_goes_only_with_a_secure_cookie() {
        use wings_browser_cookies::SameSite as Source;
        let of = |secure, same_site| made(&cookie(".a.com", secure, None, same_site), false).unwrap().same_site;
        assert_eq!(of(true, Some(Source::None)), SameSite::NoRestriction);
        assert_eq!(of(false, Some(Source::None)), SameSite::Unspecified);
        assert_eq!((of(false, Some(Source::Lax)), of(true, Some(Source::Strict)), of(true, None)), (SameSite::Lax, SameSite::Strict, SameSite::Unspecified));
    }
}
