//! On Windows, the job object that holds one Claude Code session's whole process tree (CLA-JOB-1).
//!
//! Claude Code is often started through a command shell (`claude.cmd`, as npm installs it), so the
//! process the adapter starts is not the one doing the work, and ending it leaves the session running.
//! Windows has no process group to signal as Unix has. Each session's process is placed in its own job
//! object that ends every process in it when the job is ended, or when its last handle closes: when the
//! session is stopped, when the adapter lets the process go, and when ordin itself ends however it ends,
//! because the operating system closes a process's handles as it ends.
//!
//! The calls to the operating system are the only unsafe code of this crate; each states why it holds.
#![allow(unsafe_code)]

use std::ptr;

use tokio::process::Child;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// The exit code the processes of an ended job get.
const ENDED: u32 = 1;

/// A job object that ends its processes when it is ended or its handle closes.
pub(crate) struct Job(HANDLE);

// SAFETY: a job handle is a kernel handle, valid from any thread; `Job` only passes it to calls that are
// safe to make from any thread, and closes it once, in `drop`.
unsafe impl Send for Job {}
// SAFETY: as above; `terminate` and `drop` do not touch Rust memory through the handle.
unsafe impl Sync for Job {}

impl Job {
    /// A new job holding `child`, or `None` when the operating system refuses one (the session then
    /// runs without it, and stopping it ends only the process the adapter started).
    pub(crate) fn holding(child: &Child) -> Option<Job> {
        let process = child.raw_handle()?;
        // SAFETY: no name and no security attributes: both may be null.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return None;
        }
        let job = Job(handle);
        // SAFETY: an all-zero JOBOBJECT_EXTENDED_LIMIT_INFORMATION is a valid value (no limits).
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `limits` lives across the call and the size given is its size.
        let set = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        // SAFETY: both handles are open: the job's is ours, and the child's lives while `child` does.
        if set == 0 || unsafe { AssignProcessToJobObject(job.0, process as HANDLE) } == 0 {
            return None;
        }
        Some(job)
    }

    /// Ends every process in the job at once.
    pub(crate) fn terminate(&self) {
        // SAFETY: the handle is open until `drop`.
        unsafe { TerminateJobObject(self.0, ENDED) };
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is open and closed only here. Closing the last handle ends what is left in
        // the job (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE).
        unsafe { CloseHandle(self.0) };
    }
}
