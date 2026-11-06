use gpui::{
    App, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions, point,
    prelude::*, px, size,
};
use wings_native::{
    actions,
    host::{Config, USAGE},
    icons::Assets,
    projects,
    workspace::Workspace,
};

fn run() -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let (args, restart_ack) = wings_native::updater::startup_args();
    let Some(mut config) = Config::parse(args, &cwd)? else {
        println!("{USAGE}");
        return Ok(());
    };
    // The desktop app's socket: the `wings` command finds Wings there, whichever app serves it.
    #[cfg(unix)]
    {
        // Taking it from a Wings that is still running would cut that app off from Claude Code and the `wings` command.
        config.socket = wings_connect::cli::socket()
            .ok()
            .filter(|socket| !wings_connect::server::served(socket));
    }
    // Panes run in the terminal server, where they outlive the app, when it's turned on.
    #[cfg(unix)]
    if wings_native::terminal_server::enabled()
        && let Some(data) = config.state.as_deref().and_then(std::path::Path::parent)
    {
        use wings_native::{
            host::TerminalServers,
            terminal_server::{Servers, Start, dir},
        };
        let servers = Servers::connect(&dir(data), Start::for_this_app().as_ref());
        config.terminal_server = Some(TerminalServers(std::sync::Arc::new(servers)));
    }
    // The first run starts the native project list as a copy of the desktop's.
    if let (Some(native), Some(desktop)) = (&config.projects, projects::desktop_path())
        && let Err(error) = projects::seed(native, &desktop)
    {
        eprintln!("Couldn't copy the desktop app's projects: {error}");
    }
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            actions::bind(cx);
            cx.set_menus(actions::menus());
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1000.0), px(640.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // The tabs sit in the title bar beside the traffic lights, as in the desktop app.
                    titlebar: Some(TitlebarOptions {
                        title: Some("Wings Native".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(18.0), px(13.0))),
                    }),
                    app_owns_titlebar_drag: true,
                    // The desktop's glass: what's behind the window shows through the sidebar, blurred.
                    window_background: WindowBackgroundAppearance::Blurred,
                    window_min_size: Some(size(px(640.0), px(320.0))),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Workspace::new(config, window, cx)),
            )
            .expect("Could not open the Wings native window");
            if let Some(path) = &restart_ack
                && let Err(error) = wings_native::updater::acknowledge_startup(path)
            {
                eprintln!("Could not confirm update startup: {error}");
            }
            cx.activate(true);
        });
    Ok(())
}

/// macOS keeps freed blocks in its large-allocation cache, so the scrollback hidden panes give back
/// stayed in the app's memory: 970 MB for 36 full panes, 74 MB without the cache, at the same
/// parsing speed (measured). The allocator reads the setting at launch, so the app starts itself once
/// more with it, then takes it out of the environment its shells inherit.
#[cfg(target_os = "macos")]
fn without_large_malloc_cache() {
    use std::os::unix::process::CommandExt;
    const KEY: &str = "MallocLargeCache";
    if std::env::var_os(KEY).is_some() {
        // SAFETY: no other thread runs yet.
        unsafe { std::env::remove_var(KEY) };
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let error = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .env(KEY, "0")
            .exec();
        eprintln!("Could not restart with {KEY}=0: {error}");
    }
}

fn main() {
    if let Some(result) = wings_native::updater::installer_entry() {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(2);
        }
        return;
    }
    #[cfg(target_os = "macos")]
    without_large_malloc_cache();
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
