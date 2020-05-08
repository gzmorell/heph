use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use std::{io, thread};

use crossbeam_channel::{self, Receiver};
use log::{debug, trace};
use mio::{Events, Poll, Registry, Token};

use crate::rt::hack::SetupFn;
use crate::rt::process::ProcessResult;
use crate::rt::scheduler::{LocalScheduler, SchedulerRef};
use crate::rt::timers::Timers;
use crate::rt::waker::WakerId;
use crate::rt::{channel, waker, ProcessId, RuntimeError, RuntimeInternal, RuntimeRef, Signal};

/// Message send by the coordinator thread.
#[derive(Debug)]
pub(crate) enum CoordinatorMessage {
    /// Process received a signal.
    Signal(Signal),
}

/// Message send by the worker thread.
#[derive(Debug)]
pub(crate) enum WorkerMessage {}

pub(super) struct Worker<E> {
    /// Unique id (among all threads in the `Runtime`).
    id: usize,
    /// Handle for the actual thread.
    handle: thread::JoinHandle<Result<(), RuntimeError<E>>>,
    /// Two-way communication channel to share messages with the worker thread.
    channel: channel::Handle<CoordinatorMessage, WorkerMessage>,
}

impl<E> Worker<E> {
    /// Start a new worker thread.
    pub(super) fn start<S>(
        id: usize,
        setup: Option<S>,
        scheduler_ref: SchedulerRef,
        coordinator_id: WakerId,
    ) -> io::Result<Worker<S::Error>>
    where
        S: SetupFn<Error = E>,
        E: Send + 'static,
    {
        channel::new().and_then(|(channel, worker_handle)| {
            thread::Builder::new()
                .name(format!("heph_worker{}", id))
                .spawn(move || main(setup, worker_handle, scheduler_ref, coordinator_id))
                .map(|handle| Worker {
                    id,
                    handle,
                    channel,
                })
        })
    }

    /// Return the worker's id.
    pub(super) fn id(&self) -> usize {
        self.id
    }

    pub(super) fn register(&mut self, registry: &Registry, token: Token) -> io::Result<()> {
        self.channel.register(registry, token)
    }

    /// Send the worker thread a `signal`.
    pub(super) fn send_signal(&mut self, signal: Signal) -> io::Result<()> {
        let msg = CoordinatorMessage::Signal(signal);
        self.channel.try_send(msg)
    }

    /// See [`thread::JoinHandle::join`].
    pub(super) fn join(self) -> thread::Result<Result<(), RuntimeError<E>>> {
        self.handle.join()
    }
}

/// Run the Heph runtime, with an optional `setup` function.
///
/// This is the entry point for the worker threads.
fn main<S>(
    setup: Option<S>,
    receiver: channel::Handle<WorkerMessage, CoordinatorMessage>,
    scheduler_ref: SchedulerRef,
    coordinator_id: WakerId,
) -> Result<(), RuntimeError<S::Error>>
where
    S: SetupFn,
{
    let runtime = RunningRuntime::new(receiver, scheduler_ref, coordinator_id)
        .map_err(RuntimeError::worker)?;

    // Run optional setup.
    if let Some(setup) = setup {
        let runtime_ref = runtime.create_ref();
        setup.setup(runtime_ref).map_err(RuntimeError::setup)?;
    }

    // All setup is done, so we're ready to run the event loop.
    runtime.run_event_loop()
}

/// The runtime that runs all processes.
///
/// This `pub(crate)` because it's used in the test module.
#[derive(Debug)]
pub(crate) struct RunningRuntime {
    /// Inside of the runtime, shared with zero or more `RuntimeRef`s.
    internal: Rc<RuntimeInternal>,
    events: Events,
    /// Receiving side of the channel for `Waker` events.
    waker_events: Receiver<ProcessId>,
    /// Two-way communication channel to share messages with the coordinator.
    channel: channel::Handle<WorkerMessage, CoordinatorMessage>,
}

/// Number of processes to run before polling.
///
/// This number is chosen arbitrarily, if you can improve it please do.
// TODO: find a good balance between polling, polling user space events only and
// running processes.
const RUN_POLL_RATIO: usize = 32;

/// Id used for the awakener.
const AWAKENER: Token = Token(usize::max_value());
const COORDINATOR: Token = Token(usize::max_value() - 1);

impl RunningRuntime {
    /// Create a new running runtime.
    pub(crate) fn new(
        mut channel: channel::Handle<WorkerMessage, CoordinatorMessage>,
        scheduler_ref: SchedulerRef,
        coordinator_id: WakerId,
    ) -> io::Result<RunningRuntime> {
        // System queue for event notifications.
        let poll = Poll::new()?;
        let awakener = mio::Waker::new(poll.registry(), AWAKENER)?;
        channel.register(poll.registry(), COORDINATOR)?;

        // Channel used in the `Waker` implementation.
        let (waker_sender, waker_recv) = crossbeam_channel::unbounded();
        let waker_id = waker::init(awakener, waker_sender);

        // Scheduler for scheduling and running local processes.
        let scheduler = LocalScheduler::new();

        // Internals of the running runtime.
        let internal = RuntimeInternal {
            waker_id,
            scheduler: RefCell::new(scheduler),
            scheduler_ref: RefCell::new(scheduler_ref),
            coordinator_id,
            poll: RefCell::new(poll),
            timers: RefCell::new(Timers::new()),
            signal_receivers: RefCell::new(Vec::new()),
        };

        Ok(RunningRuntime {
            internal: Rc::new(internal),
            events: Events::with_capacity(128),
            waker_events: waker_recv,
            channel,
        })
    }

    /// Create a new reference to this runtime.
    pub(crate) fn create_ref(&self) -> RuntimeRef {
        RuntimeRef {
            internal: self.internal.clone(),
        }
    }

    /// Run the runtime's event loop.
    fn run_event_loop<E>(mut self) -> Result<(), RuntimeError<E>> {
        debug!("running runtime's event loop");

        // System reference used in running the processes.
        let mut runtime_ref = self.create_ref();

        loop {
            // We first run the processes and only poll after to ensure that we
            // return if there is nothing to poll, as there would be no
            // processes to run then either.
            trace!("running processes");
            for _ in 0..RUN_POLL_RATIO {
                // NOTE: preferably this running of a process is handled
                // completely within a method of `LocalScheduler` and
                // `SchedulerRef`, however this is not possible.
                // Because we need `borrow_mut` the scheduler here we couldn't
                // also get a mutable reference to it to add actors, while a
                // process is running. Thus we need to remove a process from the
                // scheduler, drop the mutable reference, and only then run the
                // process. This allow a `RuntimeRef` to also mutable borrow the
                // `Scheduler` to add new actors to it.

                let process = self.internal.scheduler_ref.borrow_mut().try_steal();
                if let Ok(Some(mut process)) = process {
                    match process.as_mut().run(&mut runtime_ref) {
                        ProcessResult::Complete => {}
                        ProcessResult::Pending => {
                            self.internal
                                .scheduler_ref
                                .borrow_mut()
                                .add_process(process);
                        }
                    }
                    // Only run a single process per iteration.
                    continue;
                }

                let process = self.internal.scheduler.borrow_mut().next_process();
                if let Some(mut process) = process {
                    match process.as_mut().run(&mut runtime_ref) {
                        ProcessResult::Complete => {}
                        ProcessResult::Pending => {
                            self.internal.scheduler.borrow_mut().add_process(process);
                        }
                    }
                } else if !self.internal.scheduler.borrow().has_process()
                    && !self.internal.scheduler_ref.borrow().has_process()
                {
                    // No processes left to run, so we're done.
                    debug!("no processes to run, stopping runtime");
                    return Ok(());
                } else {
                    // No processes ready to run, try polling again.
                    break;
                }
            }

            self.schedule_processes().map_err(RuntimeError::worker)?;
        }
    }

    /// Schedule processes.
    ///
    /// This polls all event subsystems and schedules processes based on them.
    fn schedule_processes(&mut self) -> io::Result<()> {
        trace!("polling event sources to schedule processes");

        self.poll()?;

        let mut scheduler = self.internal.scheduler.borrow_mut();
        let mut check_coordinator = false;
        for event in self.events.iter() {
            match event.token() {
                AWAKENER => {}
                COORDINATOR => check_coordinator = true,
                token => scheduler.mark_ready(token.into()),
            }
        }

        trace!("polling wakup events");
        for pid in self.waker_events.try_iter() {
            scheduler.mark_ready(pid);
        }

        trace!("polling timers");
        for pid in self.internal.timers.borrow_mut().deadlines() {
            scheduler.mark_ready(pid);
        }

        if check_coordinator {
            drop(scheduler); // Com'on rustc, this one isn't that hard...
            self.check_coordinator()
        } else {
            Ok(())
        }
    }

    /// Poll for system events.
    fn poll(&mut self) -> io::Result<()> {
        let timeout = self.determine_timeout();

        // Only mark ourselves as polling if the timeout is non zero.
        let mark_waker = if !is_zero(timeout) {
            waker::mark_polling(self.internal.waker_id, true);
            true
        } else {
            false
        };

        let res = self
            .internal
            .poll
            .borrow_mut()
            .poll(&mut self.events, timeout);

        if mark_waker {
            waker::mark_polling(self.internal.waker_id, false);
        }

        res
    }

    /// Determine the timeout to be used in `Poll::poll`.
    fn determine_timeout(&self) -> Option<Duration> {
        // If there are any processes ready to run, any waker events or user
        // space events we don't want to block.
        if self.internal.scheduler.borrow().has_ready_process()
            || !self.waker_events.is_empty()
            || self.internal.scheduler_ref.borrow().has_ready_process()
        {
            Some(Duration::from_millis(0))
        } else if let Some(deadline) = self.internal.timers.borrow().next_deadline() {
            let now = Instant::now();
            if deadline <= now {
                // Deadline has already expired, so no blocking.
                Some(Duration::from_millis(0))
            } else {
                // Time between the deadline and right now.
                Some(deadline.duration_since(now))
            }
        } else {
            None
        }
    }

    /// Check messages from the coordinator.
    fn check_coordinator(&mut self) -> io::Result<()> {
        use CoordinatorMessage::*;
        while let Some(msg) = self.channel.try_recv()? {
            match msg {
                Signal(signal) => {
                    trace!("received process signal: {:?}", signal);
                    let mut receivers = self.internal.signal_receivers.borrow_mut();

                    if receivers.is_empty() && signal.should_stop() {
                        let msg = format!(
                            "received {:#} signal, but no receivers for it, stopping runtime",
                            signal
                        );
                        return Err(io::Error::new(io::ErrorKind::Interrupted, msg));
                    }

                    for receiver in receivers.iter_mut() {
                        // Don't care if we succeed in sending the message.
                        let _ = receiver.send(signal);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Returns `true` is timeout is `Some(Duration::from_nanos(0))`.
fn is_zero(timeout: Option<Duration>) -> bool {
    match timeout {
        Some(timeout) if timeout.subsec_nanos() == 0 => true,
        _ => false,
    }
}