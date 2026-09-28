//! Signal a process group created and owned by this host.

use std::io;

const SIGKILL: i32 = 9;
const ESRCH: i32 = 3;

unsafe extern "C" {
    fn killpg(process_group: i32, signal: i32) -> i32;
}

pub(crate) fn kill(process_group: i32) -> io::Result<()> {
    signal(process_group, SIGKILL).map(|_| ())
}

pub(crate) fn is_missing(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ESRCH)
}

fn signal(process_group: i32, signal: i32) -> io::Result<bool> {
    if unsafe { killpg(process_group, signal) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if is_missing(&error) {
        Ok(false)
    } else {
        Err(error)
    }
}
