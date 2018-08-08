//! Module containing the futures `Wake` implementation.

use std::task::{LocalWaker, Wake, local_waker_from_nonlocal};
use std::sync::Arc;

use crossbeam_channel::Sender;

use crate::process::ProcessId;

/// Create a new `LocalWaker`.
///
/// The implementation will send `ProcessId` into the `sender` channel.
pub fn new_waker(pid: ProcessId, sender: Sender<ProcessId>) -> LocalWaker {
    let waker = Arc::new(Waker {
        pid,
        sender
    });
    local_waker_from_nonlocal(waker)
}

/// The implementation behind [`new_waker`].
///
/// [`new_waker`]: fn.new_waker.html
#[derive(Debug)]
struct Waker {
    pid: ProcessId,
    sender: Sender<ProcessId>,
}

impl Wake for Waker {
    fn wake(arc_self: &Arc<Self>) {
        arc_self.sender.send(arc_self.pid);
    }
}
