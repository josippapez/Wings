//! The pure half of installing browser extensions in Wings' built-in Browser: finding extensions other browsers already
//! have, reading their manifests, unpacking store packages and working out store download URLs. The app does the HTTP
//! and hands the result to WebKit (`WKWebExtension`), which reads an unpacked folder or a ZIP.

mod detect;
mod error;
mod limits;
mod manifest;
mod store;
mod unpack;

pub use detect::{detect_installed, Installed};
pub use error::Error;
pub use manifest::{read_manifest, Info};
pub use store::{amo_file_url, store_package, Store, StorePackage};
pub use unpack::{copy_folder, unpack, MAX_FILES, MAX_TOTAL_BYTES};

#[cfg(test)]
mod tests;
