//! One site, as "Clear site data" names it: which cookies and storage are the page's, in WebKit's records and in
//! Chromium's origin, without any engine in the way so it can be tested.

/// The site a page shows: only `http` and `https` pages have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// `https://example.com:8443`: what Chromium's `Storage.clearDataForOrigin` takes.
    pub origin: String,
    /// `https://example.com:8443/`: what Chromium's cookie manager takes, the cookies a request to it would send.
    pub url: String,
    /// `example.com`, lower case, without the port or any sign-in in the address.
    pub host: String,
}

impl Site {
    /// The site of the address `address`, or none for anything but a web page (the blank page, `about:`, a file).
    pub fn of(address: &str) -> Option<Self> {
        let (scheme, rest) = address.split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return None;
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let authority = authority.rsplit_once('@').map_or(authority, |(_, after)| after).to_ascii_lowercase();
        // `[::1]:3000` keeps its brackets in the host and its port apart.
        let host = match authority.strip_prefix('[') {
            Some(inner) => format!("[{}", inner.split_once(']')?.0) + "]",
            None => authority.split(':').next().unwrap_or_default().to_owned(),
        };
        if host.is_empty() {
            return None;
        }
        Some(Self { origin: format!("{scheme}://{authority}"), url: format!("{scheme}://{authority}/"), host })
    }

    /// Whether a WebKit data record named `name` holds this site's data. WebKit names a record for the registrable
    /// domain of its sites (`example.com` for `www.example.com`, `localhost` for itself), so a record of the host's
    /// parent domain matches, and so does the host's own.
    pub fn is_in_record(&self, name: &str) -> bool {
        let name = name.trim_start_matches('.').to_ascii_lowercase();
        !name.is_empty() && (name == self.host || (name.contains('.') && self.host.ends_with(&format!(".{name}"))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_web_page_has_the_origin_and_host_of_its_address() {
        let site = Site::of("http://Localhost:4300/app/login?next=/#top").unwrap();
        assert_eq!(site, Site { origin: "http://localhost:4300".into(), url: "http://localhost:4300/".into(), host: "localhost".into() });
        let site = Site::of("https://user:pw@www.Example.com/a").unwrap();
        assert_eq!((site.origin.as_str(), site.host.as_str()), ("https://www.example.com", "www.example.com"));
        assert_eq!(Site::of("http://[::1]:3000/x").unwrap().host, "[::1]");
    }

    #[test]
    fn a_page_that_is_not_on_the_web_has_no_site() {
        for address in ["", "about:blank", "file:///tmp/a.html", "wings://start", "https://", "https:///x"] {
            assert_eq!(Site::of(address), None, "{address}");
        }
    }

    #[test]
    fn webkit_records_match_by_the_hosts_domain_and_never_another_site() {
        let site = Site::of("https://www.example.com/").unwrap();
        assert!(site.is_in_record("example.com"));
        assert!(site.is_in_record("www.example.com"));
        assert!(!site.is_in_record("other.com"));
        assert!(!site.is_in_record("notexample.com"));
        assert!(!site.is_in_record("com"));
        assert!(!site.is_in_record(""));
        let local = Site::of("http://localhost:4300/").unwrap();
        assert!(local.is_in_record("localhost"));
        assert!(!local.is_in_record("example.com"));
    }
}
