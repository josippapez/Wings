//! The helper every Chromium child process runs (renderer, GPU, network, storage, ...). Chromium starts it with
//! `--type=<kind>`; it starts the sandbox, loads the framework and hands over to CEF.
//!
//! It lives in the engine folder next to the framework, so cef-rs' loaders find the framework and the sandbox library at
//! `../../..` from the executable, as they would in an app's `Contents/Frameworks`.

fn main() {
    let args = cef::args::Args::new();

    // Before anything else: the sandbox can only start in a process that hasn't done much yet. It lets the framework
    // load from the engine folder because the browser process names that folder as the bundle the sandbox may read
    // (`wings_chromium::sandbox_bundle`).
    let mut sandbox = cef::sandbox::Sandbox::new();
    sandbox.initialize(args.as_main_args());

    let exe = std::env::current_exe().expect("no executable path");
    let library = cef::library_loader::LibraryLoader::new(&exe, true);
    if !library.load() {
        eprintln!("wings-chromium-helper: couldn't load the CEF framework next to {}", exe.display());
        std::process::exit(1);
    }
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);

    let code = cef::execute_process(Some(args.as_main_args()), None, std::ptr::null_mut());
    drop(library);
    drop(sandbox);
    std::process::exit(code);
}
