//! The terminal server owns its privacy responsibility on macOS, instead of the app that started it, and the shells it
//! runs inherit it. macOS asks for and checks Desktop, Documents, Screen Recording and the like against the process
//! responsible for a program, which is otherwise the app; once the app is gone or updated, its panes would lose them.
//! The server execs itself once at startup with `responsibility_spawnattrs_setdisclaim`, in place (`SETEXEC`, so its
//! pid stays the same), and the shells it forks then inherit the server as theirs.
//!
//! Adapted from herdr-core's `platform/macos_responsibility.rs` (Apache-2.0, see THIRD-PARTY-NOTICES).

use std::ffi::{CString, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::process::Command;

type Disclaim = unsafe extern "C" fn(*mut libc::posix_spawnattr_t, libc::c_int) -> libc::c_int;

/// Set to the server's pid on the exec that disclaimed, so it doesn't exec again.
const MARKER: &str = "_WINGS_TERMINAL_SERVER_RESPONSIBLE_PID";

/// The server argument that has a server started without launchd disclaim (`startup`). A plain child of a debug app
/// goes without: it ends with the app, and disclaimed it would need its own grant to read its binary in a protected
/// folder such as `~/Desktop`, and wait in `dyld` until someone answers the prompt.
pub const FLAG: &str = "--own-responsibility";

/// The first thing a server started without launchd but with `FLAG` does (`launchd.rs` disclaims as it detaches): exec itself
/// disclaimed, unless this is that exec. When the disclaimer fails it still execs, keeping the app's responsibility; it exits only
/// when it can't exec at all.
pub fn startup() {
    let marked = std::env::var(MARKER).ok() == Some(std::process::id().to_string());
    std::env::remove_var(MARKER);
    if marked {
        return;
    }
    let result = (|| {
        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.args(std::env::args_os().skip(1)).env(MARKER, std::process::id().to_string());
        exec(cmd)
    })();
    if let Err(error) = result {
        eprintln!("wings-terminal-server: can't disclaim the app's privacy responsibility: {error}");
        std::process::exit(1);
    }
}

/// Execs `cmd` in place as its own responsible process. `SETEXEC` leaves no short-lived wrapper as the privacy subject.
/// If the private call is missing or refused, execs it plainly rather than not starting.
pub fn exec(cmd: Command) -> io::Result<()> {
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"responsibility_spawnattrs_setdisclaim".as_ptr()) };
    let disclaim = (!symbol.is_null()).then(|| unsafe { std::mem::transmute::<*mut libc::c_void, Disclaim>(symbol) });
    exec_or_inherit(cmd, disclaim)
}

fn exec_or_inherit(mut cmd: Command, disclaim: Option<Disclaim>) -> io::Result<()> {
    match disclaim {
        Some(disclaim) => {
            let error = exec_with(&cmd, disclaim).unwrap_err();
            eprintln!("wings-terminal-server: responsibility disclaimer failed ({error}); using inherited responsibility");
        }
        None => eprintln!("wings-terminal-server: responsibility disclaimer unavailable; using inherited responsibility"),
    }
    Err(cmd.exec())
}

fn cstring(value: &OsStr) -> io::Result<CString> {
    CString::new(value.as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

fn exec_with(cmd: &Command, disclaim: Disclaim) -> io::Result<()> {
    let program = cstring(cmd.get_program())?;
    let argv: Vec<_> = std::iter::once(cmd.get_program()).chain(cmd.get_args()).map(cstring).collect::<Result<_, _>>()?;
    let mut env: std::collections::BTreeMap<_, _> = std::env::vars_os().collect();
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => {
                env.insert(key.to_owned(), value.to_owned());
            }
            None => {
                env.remove(key);
            }
        }
    }
    let env: Vec<_> = env
        .into_iter()
        .map(|(key, value)| {
            let mut pair = key;
            pair.push("=");
            pair.push(value);
            cstring(&pair)
        })
        .collect::<Result<_, _>>()?;
    let pointers = |strings: &[CString]| strings.iter().map(|s| s.as_ptr().cast_mut()).chain(std::iter::once(std::ptr::null_mut())).collect::<Vec<_>>();
    let argv = pointers(&argv);
    let env = pointers(&env);
    let check = |code| if code == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(code)) };
    unsafe {
        let mut attrs = std::mem::zeroed();
        check(libc::posix_spawnattr_init(&mut attrs))?;
        let result = (|| {
            check(libc::posix_spawnattr_setflags(&mut attrs, libc::POSIX_SPAWN_SETEXEC as libc::c_short))?;
            check(disclaim(&mut attrs, 1))?;
            check(libc::posix_spawn(std::ptr::null_mut(), program.as_ptr(), std::ptr::null(), &attrs, argv.as_ptr(), env.as_ptr()))?;
            Err(io::Error::other("SETEXEC unexpectedly returned"))
        })();
        libc::posix_spawnattr_destroy(&mut attrs);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn refuse(_: *mut libc::posix_spawnattr_t, value: i32) -> i32 {
        assert_eq!(value, 1);
        libc::EPERM
    }

    #[test]
    fn disclaimer_failure_falls_back_to_plain_exec() {
        let error = exec_or_inherit(Command::new("/nonexistent/wings-disclaim-test"), Some(refuse)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn missing_spi_uses_a_plain_exec() {
        let error = exec_or_inherit(Command::new("/nonexistent/wings-disclaim-test"), None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn the_spi_is_present_on_this_macos() {
        let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"responsibility_spawnattrs_setdisclaim".as_ptr()) };
        assert!(!symbol.is_null());
    }

    #[test]
    fn c_arguments_reject_embedded_nuls() {
        assert_eq!(cstring(OsStr::from_bytes(b"bad\0arg")).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}
