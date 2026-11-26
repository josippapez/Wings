//! The engine Wings downloads, and starting CEF from it.
//!
//! The engine is a bundle of its own, laid out like a CEF app without its executable:
//!
//! ```text
//! <engine>.app/Contents/Info.plist
//! <engine>.app/Contents/Frameworks/Chromium Embedded Framework.framework
//! <engine>.app/Contents/Frameworks/Wings Chromium Helper.app   (and " (GPU)", " (Renderer)", " (Plugin)", " (Alerts)")
//! ```
//!
//! It has to be a bundle for Chromium's child process sandbox, which only lets a child read inside one bundle
//! ([`crate::sandbox_bundle`]), and its `Info.plist` gives the bundle ID the browser process and every child agree on.
//! The helper finds the framework and the sandbox library at `../../..` from its executable, as in any CEF app, and
//! Chromium finds each helper variant by appending its suffix to the name of `browser_subprocess_path`.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use cef::{CefString, Settings};

pub const FRAMEWORK: &str = "Chromium Embedded Framework.framework";
pub const HELPER: &str = "Wings Chromium Helper";

pub struct Engine {
    bundle: PathBuf,
}

/// Where a profile keeps its data. Extensions only load into a profile on disk, not an in-memory one.
pub struct Profile {
    /// One per app: CEF locks it so two processes can't write the same profile.
    pub root: PathBuf,
    /// The profile itself, inside `root`.
    pub path: PathBuf,
}

impl Engine {
    /// `bundle`: the engine's `.app`.
    pub fn new(bundle: impl Into<PathBuf>) -> Self {
        Self { bundle: bundle.into() }
    }

    pub fn bundle(&self) -> &Path {
        &self.bundle
    }

    pub fn framework(&self) -> PathBuf {
        self.bundle.join("Contents/Frameworks").join(FRAMEWORK)
    }

    pub fn library(&self) -> PathBuf {
        self.framework().join("Chromium Embedded Framework")
    }

    pub fn helper(&self) -> PathBuf {
        self.bundle.join(format!("Contents/Frameworks/{HELPER}.app/Contents/MacOS/{HELPER}"))
    }

    /// Loads the framework from the engine. cef-rs' `LibraryLoader` looks for it inside the running app's bundle, so
    /// this calls `cef_load_library` with the absolute path instead, then declares the API version the bindings were
    /// built for, which CEF needs before any other call.
    pub fn load(&self) -> Result<(), String> {
        let library = self.library();
        if !library.is_file() {
            return Err(format!("no CEF framework at {}", library.display()));
        }
        let path = CString::new(library.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
        if cef::load_library(Some(unsafe { &*path.as_ptr() })) != 1 {
            return Err(format!("cef_load_library failed for {}", library.display()));
        }
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        Ok(())
    }

    /// The settings that start CEF from the engine, with its message loop run by the host ([`crate::pump`]).
    ///
    /// - `framework_dir_path`: without it CEF looks for the framework in the host app's `Contents/Frameworks`.
    /// - `browser_subprocess_path`: without it CEF looks for `<host> Helper.app` there too.
    /// - `main_bundle_path`: the bundle Chromium takes as the app's ("outer bundle"), for its bundle ID and the files
    ///   children read. CEF passes it on to every child; without it a helper takes its own `.app` and another bundle
    ///   ID than the browser process, and Chromium names the Mach port children connect to after that ID.
    pub fn settings(&self, profile: &Profile) -> Settings {
        Settings {
            browser_subprocess_path: string(&self.helper()),
            framework_dir_path: string(&self.framework()),
            main_bundle_path: string(&self.bundle),
            root_cache_path: string(&profile.root),
            cache_path: string(&profile.path),
            external_message_pump: 1,
            ..Default::default()
        }
    }
}

fn string(path: &Path) -> CefString {
    CefString::from(path.to_string_lossy().as_ref())
}
