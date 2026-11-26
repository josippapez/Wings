//! What WebKit keeps of an extension or a plugin apart from Wings' own folders: an extension's storage
//! (`browser.storage`, under `~/Library/WebKit/<app>/WebExtensions/<profile>/<extension>`), and a plugin's kept store of
//! cookies and site data (`WKWebsiteDataStore(forIdentifier:)`). Removing the extension or the plugin removes them too.

use std::{ptr::NonNull, rc::Rc};

use block2::RcBlock;
use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_foundation::{NSArray, NSDate, NSError, NSString, NSUUID};
use objc2_web_kit::{WKWebExtensionController, WKWebExtensionControllerConfiguration, WKWebExtensionDataRecord, WKWebsiteDataStore};

use super::{LOADED, SHARED, extensions_supported, keeps_per_plugin, unload};
use crate::web::Profile;

/// Removes what the extension `id` stored, then unloads it from `profile`'s web views, then calls `done`.
pub fn remove_extension(profile: &Profile, id: &str, done: impl Fn() + 'static) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let controller = extensions_supported().then(|| kept_controller(profile, mtm));
    let (wanted, profile, id) = (id.to_owned(), profile.clone(), id.to_owned());
    let done = Rc::new(move || {
        unload(&profile, &id);
        done();
    });
    match controller {
        Some(controller) => remove_records(&controller, move |record| record == wanted, mtm, move || done()),
        None => done(),
    }
}

/// Removes everything WebKit keeps for `profile`'s plugin, which was uninstalled: its extensions unload and their storage
/// goes, all its stores' cookies and site data go, and its kept store is deleted. A store a web view still uses can't be
/// deleted (WebKit says "in use"), but its data is gone by then.
pub fn forget_profile(profile: &Profile) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let loaded: Vec<String> =
        LOADED.with_borrow(|loaded| loaded.keys().filter(|(of, _, _)| of == profile).map(|(_, _, id)| id.clone()).collect());
    for id in loaded {
        unload(profile, &id);
    }
    // Only a plugin that had extensions: a controller made for any other would only find none.
    let had_extensions = SHARED.with_borrow(|all| all.contains_key(&(profile.clone(), true))) || profile.dir.join("extensions.json").exists();
    if extensions_supported() && had_extensions {
        remove_records(&kept_controller(profile, mtm), |_| true, mtm, || {});
    }
    let shared: Vec<_> = SHARED.with_borrow_mut(|all| {
        let gone: Vec<_> = all.keys().filter(|(of, _)| of == profile).cloned().collect();
        gone.into_iter().filter_map(|key| all.remove(&key)).collect()
    });
    // SAFETY: all of each store's kinds of data, since the start of time; WebKit calls the block on the main thread.
    unsafe {
        for shared in shared {
            shared.store.removeDataOfTypes_modifiedSince_completionHandler(
                &WKWebsiteDataStore::allWebsiteDataTypes(mtm),
                &NSDate::distantPast(),
                &RcBlock::new(|| {}),
            );
        }
    }
    // The kept store goes too, if WebKit has one by its identifier (macOS 14 has them, `keeps_per_plugin`).
    if !keeps_per_plugin() {
        return;
    }
    let (uuid, text) = (uuid(profile), profile.uuid_text());
    let found = RcBlock::new(move |all: NonNull<NSArray<NSUUID>>| {
        // SAFETY: the identifiers WebKit hands over, alive for the call.
        if !unsafe { all.as_ref() }.iter().any(|kept| kept.UUIDString().to_string().eq_ignore_ascii_case(&text)) {
            return;
        }
        let deleted = RcBlock::new(|error: *mut NSError| {
            // SAFETY: WebKit's error, alive for the call, or none.
            if let Some(error) = unsafe { error.as_ref() } {
                eprintln!("[web] couldn't delete a removed plugin's store: {}", error.localizedDescription());
            }
        });
        // SAFETY: a store by its identifier, and a block WebKit calls once on the main thread.
        unsafe { WKWebsiteDataStore::removeDataStoreForIdentifier_completionHandler(&uuid, &deleted, mtm) };
    });
    // SAFETY: a block WebKit calls once on the main thread.
    unsafe { WKWebsiteDataStore::fetchAllDataStoreIdentifiers(&found, mtm) };
}

/// The extensions' controller of `profile` that keeps their data: the one its web views use, or one made only to reach
/// that data when none has been made in this run.
fn kept_controller(profile: &Profile, mtm: MainThreadMarker) -> Retained<WKWebExtensionController> {
    if let Some(controller) = SHARED.with_borrow(|all| all.get(&(profile.clone(), true)).and_then(|shared| shared.extensions.clone())) {
        return controller;
    }
    // SAFETY: WebKit's constructors on the main thread, with the identifier the profile's own controller has (`shared`).
    unsafe {
        let configuration = WKWebExtensionControllerConfiguration::configurationWithIdentifier(&uuid(profile), mtm);
        WKWebExtensionController::initWithConfiguration(WKWebExtensionController::alloc(mtm), &configuration)
    }
}

fn uuid(profile: &Profile) -> Retained<NSUUID> {
    NSUUID::from_string(&NSString::from_str(&profile.uuid_text())).expect("a profile's UUID is valid")
}

/// Removes every kind of data the records `controller` keeps for the extensions whose ids `which` picks, then `done`.
fn remove_records(
    controller: &Retained<WKWebExtensionController>,
    which: impl Fn(&str) -> bool + 'static,
    mtm: MainThreadMarker,
    done: impl Fn() + 'static,
) {
    let done = Rc::new(done);
    // SAFETY: WebKit's own set of its kinds of extension data.
    let types = unsafe { WKWebExtensionController::allExtensionDataTypes(mtm) };
    let (remover, kinds) = (controller.clone(), types.clone());
    let fetched = RcBlock::new(move |records: NonNull<NSArray<WKWebExtensionDataRecord>>| {
        // SAFETY: the records WebKit hands over, alive for the call; their identifiers are the ids `load` gave each
        // extension's context.
        let records = unsafe { records.as_ref() };
        let picked: Vec<Retained<WKWebExtensionDataRecord>> =
            records.iter().filter(|record| which(&unsafe { record.uniqueIdentifier() }.to_string())).collect();
        if picked.is_empty() {
            return done();
        }
        let done = done.clone();
        let removed = RcBlock::new(move || done());
        // SAFETY: the records just fetched, and a block WebKit calls once on the main thread.
        unsafe { remover.removeDataOfTypes_fromDataRecords_completionHandler(&kinds, &NSArray::from_retained_slice(&picked), &removed) };
    });
    // SAFETY: a block WebKit calls once on the main thread. It finds the records of extensions that aren't loaded too.
    unsafe { controller.fetchDataRecordsOfTypes_completionHandler(&types, &fetched) };
}
