// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The connection between `kudari mcp` and a running Kudari for front ends without
//! D-Bus: a named pipe on Windows, a Unix socket elsewhere. [`serve`] answers the lines of
//! [`crate::remote`] and passes the requests to a [`Handler`]; [`connect`] opens the stream
//! for [`crate::remote::Remote`].
//!
//! Only the user may connect. On Windows, the pipe has a security descriptor that grants
//! access to the user alone and refuses clients on other computers; Kudari creates the
//! first instance of the name, so a program that took the name before fails the start
//! instead of listening. The client checks that the server runs as the same user before it
//! sends anything, and lets the server identify it but not act in its name. On Unix, the
//! socket lies in the runtime directory of the user, readable by the user alone, and both
//! sides check the user of the other one.
//!
//! One connection is one agent; the user is asked about each before it gets access.

use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::agent::{Error, Request};
use crate::mcp::Instance;
use crate::remote::{Remote, decode_request, encode_reply, read_line};

#[cfg(unix)]
use unix as system;
#[cfg(windows)]
use windows as system;

/// The stream of one connection.
pub use system::Stream;

/// What a front end does with the agents that connect.
pub trait Handler: Send + Sync {
    /// A new agent connected. `process` is the id of its process, as far as the system
    /// tells it, for the question to the user.
    fn connected(&self, agent: &str, process: Option<u32>);

    /// A request of `agent`.
    ///
    /// # Errors
    /// What the core decided, as for D-Bus.
    fn request(&self, agent: &str, request: Request) -> Result<String, Error>;

    /// The connection of `agent` ended, and with it its access.
    fn gone(&self, agent: &str);
}

/// Where the Kudari of this user listens.
///
/// # Errors
/// What the system reports about the user.
pub fn address() -> io::Result<PathBuf> {
    system::address()
}

/// Listens at `address` and answers every connection on a thread of its own, until the
/// program ends.
///
/// # Errors
/// Another program listens there already, or the system refuses.
pub fn serve(address: &Path, handler: Arc<dyn Handler>) -> io::Result<()> {
    system::serve(address, handler)
}

/// Connects to the Kudari that listens at `address`.
///
/// # Errors
/// Nothing listens there, or a program of another user does.
pub fn connect(address: &Path) -> io::Result<Stream> {
    system::connect(address)
}

/// The running Kudari as `kudari mcp` reaches it: connected at the first call, and again
/// after the connection broke, as when Kudari started anew. Each connection is a new
/// agent, which the user is asked about again.
pub struct Client {
    address: PathBuf,
    /// What an agent hears while nothing listens.
    not_running: String,
    remote: Mutex<Option<Remote<Stream>>>,
}

impl Client {
    #[must_use]
    pub fn new(address: PathBuf, not_running: &str) -> Self {
        Self {
            address,
            not_running: not_running.to_string(),
            remote: Mutex::new(None),
        }
    }

    fn call<T>(&self, call: impl FnOnce(&Remote<Stream>) -> Result<T, String>) -> Result<T, String> {
        let mut remote = self.remote.lock().unwrap_or_else(PoisonError::into_inner);
        if remote.as_ref().is_none_or(Remote::is_broken) {
            *remote = Some(Remote::new(
                connect(&self.address).map_err(|_| self.not_running.clone())?,
            ));
        }
        let Some(connected) = remote.as_ref() else {
            return Err(self.not_running.clone());
        };
        call(connected)
    }
}

impl Instance for Client {
    fn terminals(&self) -> Result<String, String> {
        self.call(Remote::terminals)
    }

    fn read_terminal(&self, terminal: i32, lines: u32) -> Result<String, String> {
        self.call(|remote| remote.read_terminal(terminal, lines))
    }

    fn send_text(&self, terminal: i32, text: &str, execute: bool) -> Result<String, String> {
        self.call(|remote| remote.send_text(terminal, text, execute))
    }

    fn send_key(&self, terminal: i32, key: &str) -> Result<(), String> {
        self.call(|remote| remote.send_key(terminal, key))
    }

    fn open_session(&self, title: &str) -> Result<String, String> {
        self.call(|remote| remote.open_session(title))
    }

    fn close_session(&self, session: i32) -> Result<(), String> {
        self.call(|remote| remote.close_session(session))
    }
}

/// A name for a new agent, unique while the program runs.
fn new_agent() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("pipe:{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Answers the requests of one agent until its connection ends.
fn answer<S>(stream: &S, process: Option<u32>, handler: &dyn Handler)
where
    for<'a> &'a S: Read + Write,
{
    let agent = new_agent();
    handler.connected(&agent, process);
    let mut reader = BufReader::new(stream);
    let mut writer = stream;
    while let Ok(Some(line)) = read_line(&mut reader) {
        if line.is_empty() {
            continue;
        }
        let reply = match decode_request(&line) {
            Ok(request) => handler.request(&agent, request),
            Err(message) => Err(Error::Invalid(message)),
        };
        if writeln!(writer, "{}", encode_reply(&reply))
            .and_then(|()| writer.flush())
            .is_err()
        {
            break;
        }
    }
    handler.gone(&agent);
}

#[cfg(unix)]
mod unix {
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::{Handler, answer};

    pub type Stream = UnixStream;

    #[allow(
        clippy::unnecessary_wraps,
        reason = "the same as on Windows, which asks for the user"
    )]
    pub fn address() -> io::Result<PathBuf> {
        let directory = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|directory| directory.is_absolute())
            .unwrap_or_else(std::env::temp_dir);
        Ok(directory.join(format!("kudari-agents-{}.socket", own_user())))
    }

    fn own_user() -> u32 {
        // SAFETY: a plain system call without arguments.
        unsafe { libc::geteuid() }
    }

    pub fn serve(address: &Path, handler: Arc<dyn Handler>) -> io::Result<()> {
        if UnixStream::connect(address).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another program listens there",
            ));
        }
        // A socket that nobody listens at is left from a Kudari that ended; anything else
        // stays, and binding fails.
        if std::fs::symlink_metadata(address).is_ok_and(|metadata| metadata.file_type().is_socket()) {
            std::fs::remove_file(address)?;
        }
        let listener = UnixListener::bind(address)?;
        std::fs::set_permissions(address, std::fs::Permissions::from_mode(0o600))?;
        std::thread::Builder::new()
            .name("agent socket".to_string())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let Some((user, process)) = peer(&stream) else {
                        continue;
                    };
                    if user != own_user() {
                        continue;
                    }
                    let handler = Arc::clone(&handler);
                    // Without a thread, the connection ends.
                    let _ = std::thread::Builder::new()
                        .name("agent".to_string())
                        .spawn(move || answer(&stream, process, &*handler));
                }
            })?;
        Ok(())
    }

    pub fn connect(address: &Path) -> io::Result<UnixStream> {
        let stream = UnixStream::connect(address)?;
        match peer(&stream) {
            Some((user, _)) if user == own_user() => Ok(stream),
            _ => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "a program of another user listens there",
            )),
        }
    }

    /// The user and the process of the other side.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn peer(stream: &UnixStream) -> Option<(u32, Option<u32>)> {
        let mut credentials = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut length = size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: the buffer is a `ucred` of the given length, for a socket that lives.
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut credentials).cast(),
                &raw mut length,
            )
        };
        (result == 0).then(|| {
            (
                credentials.uid,
                u32::try_from(credentials.pid).ok().filter(|&pid| pid != 0),
            )
        })
    }

    /// The user of the other side; these systems tell no process this way.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn peer(stream: &UnixStream) -> Option<(u32, Option<u32>)> {
        let mut user = 0;
        let mut group = 0;
        // SAFETY: plain out parameters, for a socket that lives.
        let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut user, &raw mut group) };
        (result == 0).then_some((user, None))
    }
}

#[cfg(windows)]
mod windows {
    use std::fs::File;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
        LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
        PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use super::{Handler, answer};

    pub type Stream = File;

    /// The size of the buffers of the pipe in each direction.
    const BUFFER: u32 = 64 * 1024;

    pub fn address() -> io::Result<PathBuf> {
        Ok(PathBuf::from(format!(
            r"\\.\pipe\kudari-agents-{}",
            sid_text(&own_user()?)?
        )))
    }

    pub fn serve(address: &Path, handler: Arc<dyn Handler>) -> io::Result<()> {
        let name = wide(address);
        let descriptor = Descriptor::for_user(&own_user()?)?;
        let mut pipe = instance(&name, &descriptor, true)?;
        std::thread::Builder::new()
            .name("agent pipe".to_string())
            .spawn(move || {
                loop {
                    // SAFETY: a pipe that lives, without overlapped I/O.
                    let connected = unsafe { ConnectNamedPipe(pipe.as_raw_handle(), std::ptr::null_mut()) } != 0
                        || io::Error::last_os_error().raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32);
                    // The next instance exists before this one goes, so that the name
                    // never becomes free for another program.
                    let Ok(next) = instance(&name, &descriptor, false) else {
                        return;
                    };
                    let current = std::mem::replace(&mut pipe, next);
                    if !connected {
                        continue;
                    }
                    let mut process = 0;
                    // SAFETY: a connected pipe and a plain out parameter.
                    let known = unsafe { GetNamedPipeClientProcessId(current.as_raw_handle(), &raw mut process) } != 0;
                    let process = known.then_some(process);
                    let handler = Arc::clone(&handler);
                    // Without a thread, the connection ends.
                    let _ = std::thread::Builder::new()
                        .name("agent".to_string())
                        .spawn(move || answer(&current, process, &*handler));
                }
            })?;
        Ok(())
    }

    /// A new instance of the pipe `name`; the first one must be the first of its name.
    fn instance(name: &[u16], descriptor: &Descriptor, first: bool) -> io::Result<File> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let mode = PIPE_ACCESS_DUPLEX | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
        // SAFETY: a name that ends in zero, and attributes that live for the call.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                &raw const attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new handle that nothing else owns.
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    pub fn connect(address: &Path) -> io::Result<File> {
        let name = wide(address);
        for _ in 0..10 {
            // SAFETY: a name that ends in zero. The server may identify the client, but
            // not act as it.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                // SAFETY: a new handle that nothing else owns.
                let file = unsafe { File::from_raw_handle(handle) };
                check_server(&file)?;
                return Ok(file);
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
                return Err(error);
            }
            // SAFETY: a name that ends in zero.
            unsafe { WaitNamedPipeW(name.as_ptr(), 1000) };
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "Kudari is busy"))
    }

    /// Fails unless the server of `pipe` runs as this user.
    fn check_server(pipe: &File) -> io::Result<()> {
        let mut process = 0;
        // SAFETY: a connected pipe and a plain out parameter.
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &raw mut process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let server = user_of(process)?;
        let own = own_user()?;
        // SAFETY: both buffers hold a `TOKEN_USER` with a valid SID.
        if unsafe { EqualSid(sid(&server), sid(&own)) } == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "a program of another user listens there",
            ));
        }
        Ok(())
    }

    /// The name with a zero at the end.
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    /// A `TOKEN_USER` with the SID after it, aligned for the pointers in it.
    type User = Vec<u64>;

    fn own_user() -> io::Result<User> {
        // SAFETY: the pseudo handle of this process needs no closing.
        token_user(unsafe { GetCurrentProcess() })
    }

    fn user_of(process: u32) -> io::Result<User> {
        // SAFETY: plain system calls on a handle that is checked and closed again.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process);
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let user = token_user(handle);
            CloseHandle(handle);
            user
        }
    }

    fn token_user(process: HANDLE) -> io::Result<User> {
        // SAFETY: plain system calls; the buffer holds the size that Windows asked for, and
        // the token is closed again.
        unsafe {
            let mut token = std::ptr::null_mut();
            if OpenProcessToken(process, TOKEN_QUERY, &raw mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut size = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &raw mut size);
            let mut user: User = vec![0; (size as usize).div_ceil(size_of::<u64>()).max(1)];
            let result = GetTokenInformation(token, TokenUser, user.as_mut_ptr().cast(), size, &raw mut size);
            let error = io::Error::last_os_error();
            CloseHandle(token);
            if result == 0 {
                return Err(error);
            }
            Ok(user)
        }
    }

    /// The SID in a [`User`], valid while the buffer lives.
    fn sid(user: &User) -> PSID {
        // SAFETY: the buffer starts with a `TOKEN_USER`, aligned for it.
        unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    /// The SID as text, such as `S-1-5-21-…`.
    fn sid_text(user: &User) -> io::Result<String> {
        // SAFETY: a valid SID; Windows allocates the text, which is freed again.
        unsafe {
            let mut text = std::ptr::null_mut();
            if ConvertSidToStringSidW(sid(user), &raw mut text) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
            Ok(result)
        }
    }

    /// A security descriptor that Windows allocated.
    struct Descriptor(PSECURITY_DESCRIPTOR);

    // SAFETY: the descriptor is only read once made, and freed once.
    unsafe impl Send for Descriptor {}

    impl Descriptor {
        /// Full access for `user` alone, and nothing inherited.
        fn for_user(user: &User) -> io::Result<Self> {
            let text: Vec<u16> = format!("D:P(A;;GA;;;{})", sid_text(user)?)
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let mut descriptor = std::ptr::null_mut();
            // SAFETY: a text that ends in zero and a plain out parameter.
            let result = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    SDDL_REVISION_1,
                    &raw mut descriptor,
                    std::ptr::null_mut(),
                )
            };
            if result == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(descriptor))
        }
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: Windows allocated it with `LocalAlloc`, and nothing uses it any more.
            unsafe { LocalFree(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufRead;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::remote::decode_reply;

    /// Remembers what happened, and answers the list of terminals.
    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<String>>,
    }

    impl Recorder {
        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }

        /// Waits until `count` events happened.
        fn wait_for(&self, count: usize) -> Vec<String> {
            let start = Instant::now();
            while self.events().len() < count && start.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(10));
            }
            self.events()
        }
    }

    impl Handler for Recorder {
        fn connected(&self, agent: &str, process: Option<u32>) {
            let own = process == Some(std::process::id());
            self.events.lock().unwrap().push(format!("connected {agent} own={own}"));
        }

        fn request(&self, agent: &str, request: Request) -> Result<String, Error> {
            self.events.lock().unwrap().push(format!("request {agent}"));
            match request {
                Request::Terminals => Ok("[]".to_string()),
                _ => Err(Error::Denied("Not here.".to_string())),
            }
        }

        fn gone(&self, agent: &str) {
            self.events.lock().unwrap().push(format!("gone {agent}"));
        }
    }

    /// An address of its own for each test, whose socket goes at the end.
    struct TestAddress(PathBuf);

    impl std::ops::Deref for TestAddress {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestAddress {
        fn drop(&mut self) {
            if cfg!(unix) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
    }

    fn test_address(name: &str) -> TestAddress {
        let unique = format!("kudari-test-{}-{name}", std::process::id());
        TestAddress(if cfg!(windows) {
            PathBuf::from(format!(r"\\.\pipe\{unique}"))
        } else {
            std::env::temp_dir().join(format!("{unique}.socket"))
        })
    }

    /// Whether the system names the process of a client.
    const NAMES_THE_PROCESS: bool = cfg!(any(windows, target_os = "linux", target_os = "android"));

    #[test]
    fn an_agent_talks_to_the_handler_until_it_goes() {
        let address = test_address("roundtrip");
        let recorder = Arc::new(Recorder::default());
        serve(&address, Arc::clone(&recorder) as Arc<dyn Handler>).unwrap();
        let remote = Remote::new(connect(&address).unwrap());
        assert_eq!(remote.terminals(), Ok("[]".to_string()));
        assert_eq!(remote.send_text(1, "ls", false), Err("Not here.".to_string()));
        drop(remote);
        let events = recorder.wait_for(4);
        let agent = events
            .first()
            .and_then(|event| event.split(' ').nth(1))
            .unwrap()
            .to_string();
        assert_eq!(
            events,
            [
                format!("connected {agent} own={NAMES_THE_PROCESS}"),
                format!("request {agent}"),
                format!("request {agent}"),
                format!("gone {agent}"),
            ]
        );
    }

    #[test]
    fn every_connection_is_an_agent_of_its_own() {
        let address = test_address("agents");
        let recorder = Arc::new(Recorder::default());
        serve(&address, Arc::clone(&recorder) as Arc<dyn Handler>).unwrap();
        let first = connect(&address).unwrap();
        let second = connect(&address).unwrap();
        let events = recorder.wait_for(2);
        assert_eq!(events.len(), 2);
        assert_ne!(events.first(), events.get(1));
        drop((first, second));
        assert_eq!(recorder.wait_for(4).len(), 4);
    }

    #[test]
    fn a_line_that_is_no_request_is_answered_as_invalid() {
        let address = test_address("invalid");
        serve(&address, Arc::new(Recorder::default())).unwrap();
        let stream = connect(&address).unwrap();
        let mut writer = &stream;
        writer.write_all(b"\n{\"method\":\"dance\"}\n").unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        assert_eq!(
            decode_reply(&line),
            Ok(Err(Error::Invalid("The request names no known method.".to_string())))
        );
    }

    #[test]
    fn a_second_server_cannot_take_the_address() {
        let address = test_address("taken");
        serve(&address, Arc::new(Recorder::default())).unwrap();
        assert!(serve(&address, Arc::new(Recorder::default())).is_err());
        assert!(connect(&address).is_ok(), "the first one still listens");
    }

    #[test]
    fn nothing_listens_at_an_unused_address() {
        let address = test_address("unused");
        assert!(connect(&address).is_err());
        let client = Client::new(address.to_path_buf(), "Not running.");
        assert_eq!(client.terminals(), Err("Not running.".to_string()));
    }

    #[test]
    fn the_client_connects_when_kudari_listens() {
        let address = test_address("client");
        let client = Client::new(address.to_path_buf(), "Not running.");
        assert_eq!(client.terminals(), Err("Not running.".to_string()));
        let recorder = Arc::new(Recorder::default());
        serve(&address, Arc::clone(&recorder) as Arc<dyn Handler>).unwrap();
        assert_eq!(client.terminals(), Ok("[]".to_string()));
        assert_eq!(client.terminals(), Ok("[]".to_string()));
        assert_eq!(client.read_terminal(1, 2), Err("Not here.".to_string()));
        assert_eq!(recorder.wait_for(4).len(), 4, "one connection for all calls");
        assert_eq!(
            recorder.events().first().map(|event| event.starts_with("connected")),
            Some(true)
        );
    }

    #[test]
    fn the_address_belongs_to_the_user() {
        let address = address().unwrap();
        let name = address.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("kudari-agents-"), "{name}");
        #[cfg(windows)]
        assert!(name.starts_with("kudari-agents-S-1-"), "{name}");
    }

    #[test]
    #[cfg(unix)]
    fn the_socket_is_for_the_user_alone_and_replaces_a_stale_one() {
        use std::os::unix::fs::PermissionsExt;

        let address = test_address("stale");
        drop(std::os::unix::net::UnixListener::bind(&*address).unwrap());
        serve(&address, Arc::new(Recorder::default())).unwrap();
        let mode = std::fs::metadata(&*address).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(connect(&address).is_ok());

        let file = test_address("file");
        std::fs::write(&*file, b"not a socket").unwrap();
        assert!(serve(&file, Arc::new(Recorder::default())).is_err());
        assert_eq!(std::fs::read(&*file).unwrap(), b"not a socket");
    }
}
