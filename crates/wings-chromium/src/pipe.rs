//! The DevTools protocol with the browser target, over `--remote-debugging-pipe`: Chromium reads requests from fd 3 and
//! writes answers and events to fd 4, each a JSON message ended by a NUL byte. No socket, no port: only this process
//! holds the other ends, and both fds are close-on-exec, so no program the host starts inherits them.
//!
//! It is the only in-process way to the browser target. A page's own channel (`CefBrowserHost::SendDevToolsMessage`)
//! can't load extensions: Chromium makes `Extensions.loadUnpacked` available only on the browser target
//! (`ExtensionsHandler`'s `allow_loading_extensions`, chrome_devtools_session.cc), and a page session may not attach
//! to it (`Target.attachToBrowserTarget` needs `AccessMode::kBrowser`, target_handler.cc), in Chromium 156.
//!
//! Chromium takes fd 3 and 4 by number (`kReadFD`/`kWriteFD`, devtools_agent_host_impl.cc), so [`reserve`] holds them
//! from the very start of the process, before anything else opens a file and gets them, and [`open`] swaps the pipes
//! in when CEF starts.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};

const READ_FD: RawFd = 3;
const WRITE_FD: RawFd = 4;

static RESERVED: AtomicBool = AtomicBool::new(false);

/// Holds fd 3 and 4 for the pipe, when nothing has them yet: call it first thing in `main`. A process started with
/// either already open (by whoever started it) gets no pipe.
pub fn reserve() -> bool {
    let free = |fd| unsafe { libc_fcntl(fd, F_GETFD) } == -1;
    if !free(READ_FD) || !free(WRITE_FD) {
        return false;
    }
    let null = c"/dev/null";
    // The lowest free fds are 3 and 4 now.
    let (a, b) = unsafe { (libc_open(null.as_ptr(), O_RDONLY | O_CLOEXEC), libc_open(null.as_ptr(), O_WRONLY | O_CLOEXEC)) };
    let reserved = a == READ_FD && b == WRITE_FD;
    if !reserved {
        unsafe {
            if a >= 0 {
                libc_close(a);
            }
            if b >= 0 {
                libc_close(b);
            }
        }
    }
    RESERVED.store(reserved, Ordering::Relaxed);
    reserved
}

/// The host's end of the pipe: what it writes, Chromium reads.
pub struct Pipe {
    to_chromium: File,
}

impl Pipe {
    /// Sends one message (a JSON request). Chromium answers on the reader `open` started.
    pub fn send(&mut self, message: &[u8]) -> std::io::Result<()> {
        self.to_chromium.write_all(message)?;
        self.to_chromium.write_all(&[0])
    }
}

/// Puts the pipes on fd 3 and 4 for Chromium, which takes them as it starts (`--remote-debugging-pipe`), and reads what
/// it writes on a thread of its own, handing each message to `heard` there. Call it once, before `cef::initialize`, on a
/// process [`reserve`] held them in.
pub fn open(heard: impl Fn(Vec<u8>) + Send + 'static) -> Result<Pipe, String> {
    if !RESERVED.swap(false, Ordering::Relaxed) {
        return Err("fd 3 and 4 weren't free when Wings started".into());
    }
    let (chromium_reads, host_writes) = pipe()?;
    let (host_reads, chromium_writes) = pipe()?;
    unsafe {
        if libc_dup2(chromium_reads, READ_FD) != READ_FD || libc_dup2(chromium_writes, WRITE_FD) != WRITE_FD {
            return Err("couldn't move the DevTools pipe to fd 3 and 4".into());
        }
        // dup2 clears close-on-exec, and a terminal that inherited these could drive the browser.
        libc_fcntl_set(READ_FD, F_SETFD, FD_CLOEXEC);
        libc_fcntl_set(WRITE_FD, F_SETFD, FD_CLOEXEC);
        libc_close(chromium_reads);
        libc_close(chromium_writes);
    }
    let mut from_chromium = unsafe { File::from_raw_fd(host_reads) };
    std::thread::Builder::new()
        .name("chromium-devtools-pipe".into())
        .spawn(move || {
            let mut pending = Vec::new();
            let mut chunk = [0u8; 64 * 1024];
            loop {
                let read = match from_chromium.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                pending.extend_from_slice(&chunk[..read]);
                for message in split(&mut pending) {
                    heard(message);
                }
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(Pipe { to_chromium: unsafe { File::from_raw_fd(host_writes) } })
}

/// Takes every whole message (ended by a NUL) off the front of `pending`, leaving the start of the next one.
pub fn split(pending: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut messages = Vec::new();
    while let Some(end) = pending.iter().position(|byte| *byte == 0) {
        let rest = pending.split_off(end + 1);
        pending.pop();
        messages.push(std::mem::replace(pending, rest));
    }
    messages
}

fn pipe() -> Result<(RawFd, RawFd), String> {
    let mut fds = [0 as RawFd; 2];
    if unsafe { libc_pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    for fd in fds {
        unsafe { libc_fcntl_set(fd, F_SETFD, FD_CLOEXEC) };
    }
    Ok((fds[0], fds[1]))
}

const F_GETFD: i32 = 1;
const F_SETFD: i32 = 2;
const FD_CLOEXEC: i32 = 1;
const O_RDONLY: i32 = 0;
const O_WRONLY: i32 = 1;
const O_CLOEXEC: i32 = 0x0100_0000;

extern "C" {
    #[link_name = "fcntl"]
    fn libc_fcntl(fd: i32, cmd: i32, ...) -> i32;
    #[link_name = "open"]
    fn libc_open(path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    #[link_name = "close"]
    fn libc_close(fd: i32) -> i32;
    #[link_name = "dup2"]
    fn libc_dup2(from: i32, to: i32) -> i32;
    #[link_name = "pipe"]
    fn libc_pipe(fds: *mut i32) -> i32;
}

unsafe fn libc_fcntl_set(fd: i32, cmd: i32, value: i32) -> i32 {
    unsafe { libc_fcntl(fd, cmd, value) }
}

#[cfg(test)]
mod tests {
    use super::split;

    /// Chromium's messages come in chunks of any size, each ended by a NUL.
    #[test]
    fn messages_are_split_at_each_nul_and_a_partial_one_waits_for_the_rest() {
        let mut pending = b"{\"id\":1}\0{\"id\":2}\0{\"id\"".to_vec();
        assert_eq!(split(&mut pending), [b"{\"id\":1}".to_vec(), b"{\"id\":2}".to_vec()]);
        assert_eq!(pending, b"{\"id\"");
        pending.extend_from_slice(b":3}\0");
        assert_eq!(split(&mut pending), [b"{\"id\":3}".to_vec()]);
        assert!(pending.is_empty());
    }
}
