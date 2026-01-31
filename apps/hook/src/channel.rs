//! F. The authenticated local channel to the core: connect, one request line, one or two reply lines.
//!
//! Reads happen on a helper thread so that every wait has a deadline on every platform (a Windows pipe
//! opened as a file has no read timeout).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_types::time::Timestamp;
use ordin_types::wire::hook::HookReply;
use ordin_types::HookCallId;

use crate::error::HookFailure;

/// The longest reply line read (chosen here): a reply is a few hundred bytes.
pub const REPLY_LINE_MAX: u64 = 64 * 1024;

type Shutdowner = Arc<dyn Fn(Shutdown) + Send + Sync>;

/// An open connection, split into its halves.
pub struct Conn {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub shutdown: Shutdowner,
}

/// Connects to the endpoint within `timeout`. Failure is `HOOK_CORE_UNREACHABLE`.
pub fn connect(endpoint: &str, timeout: Duration) -> Result<Conn, HookFailure> {
    let (tx, rx) = mpsc::channel();
    let endpoint = endpoint.to_string();
    std::thread::spawn(move || {
        // The receiver may have gone after a timeout; the connection is then simply dropped.
        let _ = tx.send(open(&endpoint, timeout));
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(conn)) => Ok(conn),
        _ => Err(HookFailure::core_unreachable()),
    }
}

#[cfg(unix)]
fn open(endpoint: &str, _timeout: Duration) -> std::io::Result<Conn> {
    let stream = std::os::unix::net::UnixStream::connect(endpoint)?;
    let reader = stream.try_clone()?;
    let closer = stream.try_clone()?;
    Ok(Conn {
        reader: Box::new(reader),
        writer: Box::new(stream),
        shutdown: Arc::new(move |how| {
            let _ = closer.shutdown(how);
        }),
    })
}

// spec: ordin-desktop/components/OrdinHook#SA-HOOK-PIPE-01
#[cfg(windows)]
fn open(endpoint: &str, timeout: Duration) -> std::io::Result<Conn> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OVERLAPPED;
    let started = Instant::now();
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .open(endpoint)
        {
            Ok(file) => return pipe::split(file),
            // Busy pipe (ERROR_PIPE_BUSY = 231): retry every 50 ms until the timeout.
            Err(e) if e.raw_os_error() == Some(231) && started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

/// The client end of a Windows pipe, opened for overlapped I/O and split into a reading and a
/// writing half that work at the same time.
///
/// A pipe handle opened for synchronous I/O serialises its operations: a read waiting for the core's
/// next bytes holds up every write on the same file object, duplicates included, so the bridge's two
/// copying threads would stall each other. Overlapped operations do not queue behind each other.
#[cfg(windows)]
mod pipe {
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::sync::Arc;

    use windows_sys::Win32::Foundation::{
        ERROR_BROKEN_PIPE, ERROR_IO_PENDING, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

    use super::Conn;

    /// A manual-reset event, not signalled.
    fn event() -> std::io::Result<OwnedHandle> {
        // SAFETY: null attributes and a null name ask for an unnamed event with the default descriptor.
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `handle` is a valid event handle owned by nothing else.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    fn closed_error() -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            "the connection was closed",
        )
    }

    /// One direction of the connection. The pipe handle is shared; it is released when both halves
    /// have gone, which is the end of the connection the core reads.
    struct Half {
        pipe: Arc<std::fs::File>,
        /// Signalled by `shutdown(Both)`: every operation pending or still to come ends.
        closed: Arc<OwnedHandle>,
        /// The completion event of this half's one operation at a time.
        done: OwnedHandle,
    }

    impl Half {
        /// Starts one overlapped operation with `start` and waits until it completes or the
        /// connection is closed. A pending operation is cancelled and waited for, so it never outlives
        /// the buffer it was given.
        // spec: ordin-desktop/components/OrdinHook#SA-HOOK-PIPE-03
        fn transfer(
            &mut self,
            start: impl FnOnce(HANDLE, *mut OVERLAPPED) -> i32,
        ) -> std::io::Result<usize> {
            let pipe = self.pipe.as_raw_handle();
            let closed = self.closed.as_raw_handle();
            // SAFETY: `closed` is a valid event handle for the life of `self`.
            if unsafe { WaitForSingleObject(closed, 0) } == WAIT_OBJECT_0 {
                return Err(closed_error());
            }
            // SAFETY: OVERLAPPED is plain data; all zeroes is its documented initial state.
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
            overlapped.hEvent = self.done.as_raw_handle();
            if start(pipe, &mut overlapped) == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                    return Err(error);
                }
            }
            let events = [overlapped.hEvent, closed];
            // SAFETY: both handles are valid events for the duration of the wait.
            let woken = unsafe { WaitForMultipleObjects(2, events.as_ptr(), 0, INFINITE) };
            if woken != WAIT_OBJECT_0 {
                // SAFETY: `overlapped` is the operation started above on `pipe`.
                unsafe { CancelIoEx(pipe, &overlapped) };
            }
            let mut moved = 0u32;
            // SAFETY: waiting (bWait = 1) returns only once the operation has completed or been
            // cancelled, after which the system no longer touches `overlapped` or the buffer.
            if unsafe { GetOverlappedResult(pipe, &overlapped, &mut moved, 1) } == 0 {
                let error = std::io::Error::last_os_error();
                return Err(if woken == WAIT_OBJECT_0 {
                    error
                } else {
                    closed_error()
                });
            }
            Ok(moved as usize)
        }
    }

    impl Read for Half {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }
            let len = buf.len().min(u32::MAX as usize) as u32;
            let at = buf.as_mut_ptr();
            // SAFETY: `at` is valid for `len` bytes until `transfer` returns, which is after the
            // operation has completed or been cancelled.
            let read = self.transfer(|pipe, overlapped| unsafe {
                ReadFile(pipe, at, len, std::ptr::null_mut(), overlapped)
            });
            match read {
                // The core closed its end: end of file, as a socket reports it.
                Err(e) if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
                other => other,
            }
        }
    }

    impl Write for Half {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }
            let len = buf.len().min(u32::MAX as usize) as u32;
            let at = buf.as_ptr();
            // SAFETY: as for `read`; the buffer is only read.
            self.transfer(|pipe, overlapped| unsafe {
                WriteFile(pipe, at, len, std::ptr::null_mut(), overlapped)
            })
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Splits an overlapped pipe handle into the connection's two halves.
    ///
    /// A pipe client cannot close one direction only, so `shutdown(Write)` does nothing here: the
    /// bridge closes the whole connection after its drain (G.4), and that close is the end of input
    /// the MCP Front reads. `shutdown(Both)` ends every pending operation, so both halves are dropped
    /// and the handle is released.
    // spec: ordin-desktop/components/OrdinHook#SA-HOOK-PIPE-02
    pub(super) fn split(file: std::fs::File) -> std::io::Result<Conn> {
        let pipe = Arc::new(file);
        let closed = Arc::new(event()?);
        let reader = Half {
            pipe: pipe.clone(),
            closed: closed.clone(),
            done: event()?,
        };
        let writer = Half {
            pipe,
            closed: closed.clone(),
            done: event()?,
        };
        Ok(Conn {
            reader: Box::new(reader),
            writer: Box::new(writer),
            shutdown: Arc::new(move |how| {
                if how == Shutdown::Both {
                    // SAFETY: `closed` is a valid event handle held by this closure.
                    unsafe { SetEvent(closed.as_raw_handle()) };
                }
            }),
        })
    }
}

/// One line read from `reader`, at most `REPLY_LINE_MAX` bytes. `Ok(None)` is end of file.
pub fn read_line<R: BufRead>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let n = reader
        .by_ref()
        .take(REPLY_LINE_MAX)
        .read_until(b'\n', &mut bytes)?;
    if n == 0 {
        return Ok(None);
    }
    if bytes.last() != Some(&b'\n') && n as u64 >= REPLY_LINE_MAX {
        return Err(std::io::Error::other("line too long"));
    }
    String::from_utf8(bytes)
        .map(|s| Some(s.trim_end_matches(['\n', '\r']).to_string()))
        .map_err(std::io::Error::other)
}

/// Reads one line within `timeout`, handing the buffered reader back so no byte is lost.
pub fn read_line_within(
    reader: Box<dyn Read + Send>,
    timeout: Duration,
) -> Option<(String, BufReader<Box<dyn Read + Send>>)> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffered = BufReader::new(reader);
        if let Ok(Some(line)) = read_line(&mut buffered) {
            let _ = tx.send((line, buffered));
        }
    });
    rx.recv_timeout(timeout).ok()
}

enum Line {
    Text(String),
    Closed,
}

/// Sends `request_line` (without its newline) and returns the final reply for `call_id`.
///
/// `first_timeout` bounds the wait for the first reply; `deadline` bounds the whole exchange.
pub fn exchange(
    conn: Conn,
    request_line: &[u8],
    call_id: HookCallId,
    deadline: Instant,
    first_timeout: Duration,
) -> Result<HookReply, HookFailure> {
    let Conn {
        reader, mut writer, ..
    } = conn;
    let write = writer
        .write_all(request_line)
        .and_then(|_| writer.write_all(b"\n"))
        .and_then(|_| writer.flush());
    if write.is_err() {
        return Err(HookFailure::core_unreachable());
    }
    let sent = Instant::now();
    let (tx, rx) = mpsc::channel::<Line>();
    std::thread::spawn(move || {
        let mut buffered = BufReader::new(reader);
        loop {
            let line = match read_line(&mut buffered) {
                Ok(Some(text)) => Line::Text(text),
                _ => Line::Closed,
            };
            let closed = matches!(line, Line::Closed);
            if tx.send(line).is_err() || closed {
                break;
            }
        }
    });

    let until = (sent + first_timeout).min(deadline);
    let first = match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
        Ok(Line::Text(text)) => parse_reply(&text, call_id)?,
        Ok(Line::Closed) | Err(RecvTimeoutError::Disconnected) => {
            return Err(HookFailure::reply_malformed())
        }
        Err(RecvTimeoutError::Timeout) => return Err(HookFailure::core_timeout()),
    };
    let HookReply::Holding { deadline: d, .. } = first else {
        return Ok(first);
    };
    let hold = Duration::from_millis((d.0 - Timestamp::now().0).num_milliseconds().max(0) as u64);
    let until = (Instant::now() + hold).min(deadline);
    match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
        Ok(Line::Text(text)) => match parse_reply(&text, call_id)? {
            HookReply::Holding { .. } => Err(HookFailure::reply_malformed()),
            other => Ok(other),
        },
        Ok(Line::Closed) | Err(RecvTimeoutError::Disconnected) => {
            Err(HookFailure::reply_malformed())
        }
        Err(RecvTimeoutError::Timeout) => Err(HookFailure::approval_pending()),
    }
}

fn reply_call_id(reply: &HookReply) -> HookCallId {
    match reply {
        HookReply::Holding { call_id, .. }
        | HookReply::Allow { call_id }
        | HookReply::Deny { call_id, .. }
        | HookReply::Ack { call_id } => *call_id,
    }
}

fn parse_reply(text: &str, call_id: HookCallId) -> Result<HookReply, HookFailure> {
    let reply: HookReply =
        serde_json::from_str(text).map_err(|_| HookFailure::reply_malformed())?;
    if reply_call_id(&reply) != call_id {
        return Err(HookFailure::reply_malformed());
    }
    Ok(reply)
}
