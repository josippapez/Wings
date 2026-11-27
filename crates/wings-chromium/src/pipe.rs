//! The DevTools protocol with the browser target, over `--remote-debugging-pipe`: Chromium reads requests from fd 3 and
//! writes answers and events to fd 4, each a JSON message ended by a NUL byte. No socket, no port: only this process
//! holds the other ends, and every end is close-on-exec, so no program the host starts inherits one.
//!
//! It is the only in-process way to the browser target. A page's own channel (`CefBrowserHost::SendDevToolsMessage`)
//! can't load extensions: Chromium makes `Extensions.loadUnpacked` available only on the browser target
//! (`ExtensionsHandler`'s `allow_loading_extensions`, chrome_devtools_session.cc), and a page session may not attach
//! to it (`Target.attachToBrowserTarget` needs `AccessMode::kBrowser`, target_handler.cc), in Chromium 156.
//!
//! Chromium takes fd 3 and 4 by number (`kReadFD`/`kWriteFD`, `StartRemoteDebuggingPipeHandler` in
//! devtools_agent_host_impl.cc) and reads and writes them in the browser process, which is the host's own, so
//! close-on-exec doesn't touch it. [`reserve`] makes both pipes at the very start of the process, before anything else
//! opens a file and takes those fds, and while it has one thread: macOS has no `pipe2`, so an end is close-on-exec only a
//! call after it is made, and a thread that started a program in between would hand it the end. [`open`] then only takes
//! Wings' ends when CEF starts.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::sync::Mutex;

const READ_FD: RawFd = 3;
const WRITE_FD: RawFd = 4;

/// The host's ends of the pipes [`reserve`] made, until [`open`] takes them.
static RESERVED: Mutex<Option<Ends>> = Mutex::new(None);

/// The host's ends of the two pipes whose other ends are on Chromium's fds.
struct Ends {
    /// What Chromium reads, the host writes here.
    host_writes: RawFd,
    /// What Chromium writes, the host reads here.
    host_reads: RawFd,
}

/// Makes the pipe on fd 3 and 4: call it first thing in `main`, while the process has one thread.
pub fn reserve() -> bool {
    let ends = reserve_at(READ_FD, WRITE_FD);
    let reserved = ends.is_some();
    *RESERVED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = ends;
    reserved
}

/// [`reserve_on`], after moving whatever the process was started with on `read_fd` or `write_fd` to a higher fd. Wings
/// restarted by its updater starts with the old copy's sockets there (no code here knows them by number), and with those
/// in the way Chromium got no pipe, so no extension loaded in that run.
fn reserve_at(read_fd: RawFd, write_fd: RawFd) -> Option<Ends> {
    for fd in [read_fd, write_fd] {
        if unsafe { libc_fcntl(fd, F_GETFD) } != -1 {
            let moved = unsafe { libc_fcntl_set(fd, F_DUPFD_CLOEXEC, read_fd.max(write_fd) + 1) };
            if moved == -1 {
                return None;
            }
            unsafe { libc_close(fd) };
        }
    }
    reserve_on(read_fd, write_fd)
}

/// Both pipes, with Chromium's ends on `read_fd` and `write_fd` and every end close-on-exec from the call after it was
/// made, or none if either fd is taken or a step fails.
fn reserve_on(read_fd: RawFd, write_fd: RawFd) -> Option<Ends> {
    let free = |fd| unsafe { libc_fcntl(fd, F_GETFD) } == -1;
    if !free(read_fd) || !free(write_fd) {
        return None;
    }
    // Every end above both fds, so neither pipe sits on one the other's end is to go to.
    let above = read_fd.max(write_fd) + 1;
    let (chromium_reads, host_writes) = pipe_above(above)?;
    let Some((host_reads, chromium_writes)) = pipe_above(above) else {
        close_all(&[chromium_reads, host_writes]);
        return None;
    };
    // dup2 makes a copy without close-on-exec: it is set again the next call, with no other thread to start a program.
    let placed = unsafe {
        libc_dup2(chromium_reads, read_fd) == read_fd
            && libc_fcntl_set(read_fd, F_SETFD, FD_CLOEXEC) == 0
            && libc_dup2(chromium_writes, write_fd) == write_fd
            && libc_fcntl_set(write_fd, F_SETFD, FD_CLOEXEC) == 0
    };
    close_all(&[chromium_reads, chromium_writes]);
    if !placed {
        // Both were free, so whatever is on them now is this call's.
        close_all(&[read_fd, write_fd, host_writes, host_reads]);
        return None;
    }
    Some(Ends { host_writes, host_reads })
}

/// A pipe, its read end then its write end, both close-on-exec and at `min` or above.
fn pipe_above(min: RawFd) -> Option<(RawFd, RawFd)> {
    let mut fds = [0 as RawFd; 2];
    if unsafe { libc_pipe(fds.as_mut_ptr()) } != 0 {
        return None;
    }
    // Each moved at once, close-on-exec as it is copied (`F_DUPFD_CLOEXEC`), and the one `pipe` made closed.
    let moved = fds.map(|fd| unsafe {
        let moved = libc_fcntl_set(fd, F_DUPFD_CLOEXEC, min);
        libc_close(fd);
        moved
    });
    if moved.contains(&-1) {
        close_all(&moved.into_iter().filter(|fd| *fd >= 0).collect::<Vec<_>>());
        return None;
    }
    Some((moved[0], moved[1]))
}

fn close_all(fds: &[RawFd]) {
    for fd in fds {
        unsafe { libc_close(*fd) };
    }
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

/// Takes the host's ends of the pipe [`reserve`] made, for Chromium to take its own as it starts
/// (`--remote-debugging-pipe`), and reads what it writes on a thread of its own, handing each message to `heard` there.
/// Call it once, before `cef::initialize`.
pub fn open(heard: impl Fn(Vec<u8>) + Send + 'static) -> Result<Pipe, String> {
    let ends = RESERVED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    hand_over(ends.ok_or("fd 3 and 4 weren't free when Wings started")?, heard)
}

fn hand_over(ends: Ends, heard: impl Fn(Vec<u8>) + Send + 'static) -> Result<Pipe, String> {
    let mut from_chromium = unsafe { File::from_raw_fd(ends.host_reads) };
    let to_chromium = unsafe { File::from_raw_fd(ends.host_writes) };
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
    Ok(Pipe { to_chromium })
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

// macOS's sys/fcntl.h.
const F_GETFD: i32 = 1;
const F_SETFD: i32 = 2;
const F_DUPFD_CLOEXEC: i32 = 67;
const FD_CLOEXEC: i32 = 1;

extern "C" {
    #[link_name = "fcntl"]
    fn libc_fcntl(fd: i32, cmd: i32, ...) -> i32;
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
    use super::*;

    fn close_on_exec(fd: RawFd) -> bool {
        let flags = unsafe { libc_fcntl(fd, F_GETFD) };
        flags != -1 && flags & FD_CLOEXEC != 0
    }

    /// Both pipes exist as soon as the fds are reserved, every end close-on-exec already, so no program started later
    /// gets one; Chromium's ends carry what Wings' ends send and read, and `hand_over` reads Chromium's messages.
    /// Fds far above any the test opens stand in for 3 and 4.
    #[test]
    fn reserving_makes_both_pipes_with_every_end_close_on_exec_and_hands_over_only_wings_ends() {
        let (read_fd, write_fd) = (900, 901);
        let ends = reserve_on(read_fd, write_fd).expect("900 and 901 are free");
        for fd in [read_fd, write_fd, ends.host_writes, ends.host_reads] {
            assert!(close_on_exec(fd), "fd {fd} isn't close-on-exec");
        }
        assert!(![ends.host_writes, ends.host_reads].iter().any(|fd| [read_fd, write_fd].contains(fd)));
        // Taken now: a reserve on either gets nothing, and opens nothing.
        assert!(reserve_on(read_fd, 951).is_none());
        assert!(reserve_on(950, write_fd).is_none());
        assert!([950, 951].iter().all(|fd| unsafe { libc_fcntl(*fd, F_GETFD) } == -1));

        let mut chromium_reads = unsafe { File::from_raw_fd(read_fd) };
        let mut chromium_writes = unsafe { File::from_raw_fd(write_fd) };
        let (heard, messages) = std::sync::mpsc::channel();
        let mut pipe = hand_over(ends, move |message| drop(heard.send(message))).unwrap();
        pipe.send(b"{\"id\":1}").unwrap();
        let mut sent = [0u8; 9];
        chromium_reads.read_exact(&mut sent).unwrap();
        assert_eq!(&sent, b"{\"id\":1}\0");
        chromium_writes.write_all(b"{\"id\":1,\"result\":{}}\0").unwrap();
        let wait = std::time::Duration::from_secs(5);
        assert_eq!(messages.recv_timeout(wait).unwrap(), b"{\"id\":1,\"result\":{}}");
    }

    /// A process started with something already on the pipe's fds (Wings after its updater restarts it) still gets the
    /// pipe: what was there moves to a higher fd, open and close-on-exec.
    #[test]
    fn what_the_process_started_with_on_the_pipes_fds_moves_aside() {
        use std::os::unix::fs::MetadataExt;
        let (read_fd, write_fd) = (960, 961);
        let path = std::env::temp_dir().join(format!("wings-pipe-test-{}", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let inode = file.metadata().unwrap().ino();
        let inherited = std::os::fd::AsRawFd::as_raw_fd(&file);
        assert_eq!(unsafe { libc_dup2(inherited, read_fd) }, read_fd);
        assert_eq!(unsafe { libc_dup2(inherited, write_fd) }, write_fd);
        let ends = reserve_at(read_fd, write_fd).expect("the inherited fds move aside");
        for fd in [read_fd, write_fd, ends.host_writes, ends.host_reads] {
            assert!(close_on_exec(fd), "fd {fd} isn't close-on-exec");
        }
        // Both inherited copies stay open above the pipe's fds, close-on-exec.
        let moved: Vec<RawFd> = (write_fd + 1..write_fd + 200)
            .filter(|fd| std::fs::metadata(format!("/dev/fd/{fd}")).is_ok_and(|meta| meta.ino() == inode))
            .collect();
        assert_eq!(moved.len(), 2, "{moved:?}");
        assert!(moved.iter().all(|fd| close_on_exec(*fd)));
    }

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
