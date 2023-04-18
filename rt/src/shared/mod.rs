//! Module with shared runtime internals.

use std::cmp::min;
use std::future::Future;
use std::os::fd::{AsFd, AsRawFd, RawFd};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};
use std::{io, task};

use heph::actor::{ActorFuture, NewActor};
use heph::actor_ref::ActorRef;
use heph::supervisor::Supervisor;
use log::{as_debug, debug, error, trace};
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Registry, Token};

use crate::process::{FutureProcess, Process, ProcessId};
use crate::spawn::{ActorOptions, FutureOptions};
use crate::thread_waker::ThreadWaker;
use crate::timers::TimerToken;
use crate::{trace, ThreadSafe};

mod scheduler;
pub(crate) mod waker;

use crate::timers::shared::Timers;
use scheduler::{ProcessData, Scheduler};
use waker::WakerId;

/// Setup of [`RuntimeInternals`].
///
/// # Notes
///
/// This type only exists because [`Arc::new_cyclic`] doesn't work when
/// returning a result. And as [`RuntimeInternals`] needs to create a [`Poll`]
/// instance, which can fail, creating a new `RuntimeInternals` inside
/// `Arc::new_cyclic` doesn't work. So it needs to be a two step process, where
/// the second step (`RuntimeSetup::complete`) doesn't return an error and can
/// be called inside `Arc::new_cyclic`.
pub(crate) struct RuntimeSetup {
    poll: Poll,
    ring: a10::Ring,
}

impl RuntimeSetup {
    /// Complete the runtime setup.
    pub(crate) fn complete(
        self,
        shared_id: WakerId,
        worker_wakers: Box<[&'static ThreadWaker]>,
        trace_log: Option<Arc<trace::SharedLog>>,
    ) -> RuntimeInternals {
        // Needed by `RuntimeInternals::wake_workers`.
        debug_assert!(worker_wakers.len() >= 1);
        let sq = self.ring.submission_queue().clone();
        RuntimeInternals {
            shared_id,
            worker_wakers,
            wake_worker_idx: AtomicUsize::new(0),
            poll: Mutex::new(self.poll),
            ring: Mutex::new(self.ring),
            sq,
            scheduler: Scheduler::new(),
            timers: Timers::new(),
            trace_log,
        }
    }
}

/// Shared internals of the runtime.
#[derive(Debug)]
pub(crate) struct RuntimeInternals {
    /// Waker id used to create [`task::Waker`]s for thread-safe actors.
    shared_id: WakerId,
    /// Thread wakers for all the workers.
    worker_wakers: Box<[&'static ThreadWaker]>,
    /// Index into `worker_wakers` to wake next, see
    /// [`RuntimeInternals::wake_workers`].
    wake_worker_idx: AtomicUsize,
    /// Poll instance for all shared event sources. This is polled by the worker
    /// thread.
    poll: Mutex<Poll>,
    /// io_uring completion ring.
    ring: Mutex<a10::Ring>,
    /// SubmissionQueue for the `ring`.
    sq: a10::SubmissionQueue,
    /// Scheduler for thread-safe actors.
    scheduler: Scheduler,
    /// Timers for thread-safe actors.
    timers: Timers,
    /// Shared trace log.
    ///
    /// # Notes
    ///
    /// Prefer not to use this but use [`trace::Log`] in local internals
    /// instead.
    trace_log: Option<Arc<trace::SharedLog>>,
}

/// Metrics for [`RuntimeInternals`].
#[derive(Debug)]
pub(crate) struct Metrics {
    pub(crate) scheduler_ready: usize,
    pub(crate) scheduler_inactive: usize,
    pub(crate) timers_total: usize,
    pub(crate) timers_next: Option<Duration>,
}

impl RuntimeInternals {
    /// Setup new runtime internals.
    pub(crate) fn setup(coordinator_sq: &a10::SubmissionQueue) -> io::Result<RuntimeSetup> {
        let poll = Poll::new()?;
        let ring = a10::Ring::config(512)
            .attach_queue(coordinator_sq)
            .build()?;
        Ok(RuntimeSetup { poll, ring })
    }

    /// Same as [`setup`], but doesn't attach to an existing [`a10::Ring`].
    #[cfg(any(test, feature = "test"))]
    pub(crate) fn test_setup() -> io::Result<RuntimeSetup> {
        let poll = Poll::new()?;
        let ring = a10::Ring::new(512)?;
        Ok(RuntimeSetup { poll, ring })
    }

    /// Returns metrics about the shared scheduler and timers.
    pub(crate) fn metrics(&self) -> Metrics {
        Metrics {
            scheduler_ready: self.scheduler.ready(),
            scheduler_inactive: self.scheduler.inactive(),
            timers_total: self.timers.len(),
            timers_next: self.timers.next_timer(),
        }
    }

    /// Returns a new [`task::Waker`] for the thread-safe actor with `pid`.
    pub(crate) fn new_task_waker(&self, pid: ProcessId) -> task::Waker {
        waker::new(self.shared_id, pid)
    }

    /// Register the shared [`Poll`] instance with `registry`.
    pub(crate) fn register_worker_poll(&self, registry: &Registry, token: Token) -> io::Result<()> {
        use mio::event::Source;
        let poll = self.poll.lock().unwrap();
        SourceFd(&poll.as_raw_fd()).register(registry, token, Interest::READABLE)
    }

    /// Returns `Ok(true)` if it polled the shared [`Poll`] instance, writing OS
    /// events to `events`. Returns `Ok(false)` if another worker is currently
    /// polling, which means this worker doesn't have to anymore. Otherwise it
    /// returns an error.
    pub(crate) fn try_poll(&self, events: &mut Events) -> io::Result<bool> {
        match self.poll.try_lock() {
            Ok(mut poll) => poll.poll(events, Some(Duration::ZERO)).map(|()| true),
            Err(TryLockError::WouldBlock) => Ok(false),
            Err(TryLockError::Poisoned(err)) => panic!("failed to lock shared poll: {err}"),
        }
    }

    /// Polls the io_uring completion ring if it's currently not being polled.
    pub(crate) fn try_poll_ring(&self) -> io::Result<()> {
        match self.ring.try_lock() {
            Ok(mut ring) => ring.poll(Some(Duration::ZERO)),
            Err(TryLockError::WouldBlock) => Ok(()),
            Err(TryLockError::Poisoned(err)) => panic!("failed to lock shared io_uring: {err}"),
        }
    }

    /// Return the file descriptor for the io_uring.
    pub(crate) fn ring_fd(&self) -> RawFd {
        self.ring.lock().unwrap().as_fd().as_raw_fd()
    }

    /// Returns the io_uring submission queue.
    pub(crate) fn submission_queue(&self) -> &a10::SubmissionQueue {
        &self.sq
    }

    /// Add a timer.
    ///
    /// See [`Timers::add`].
    pub(crate) fn add_timer(&self, deadline: Instant, waker: task::Waker) -> TimerToken {
        trace!(deadline = as_debug!(deadline); "adding timer");
        self.timers.add(deadline, waker)
    }

    /// Remove a previously set timer.
    ///
    /// See [`Timers::remove`].
    pub(crate) fn remove_timer(&self, deadline: Instant, token: TimerToken) {
        trace!(deadline = as_debug!(deadline); "removing timer");
        self.timers.remove(deadline, token);
    }

    /// Wake all futures who's timers has expired.
    ///
    /// See [`Timers::expire_timers`].
    pub(crate) fn expire_timers(&self, now: Instant) -> usize {
        self.timers.expire_timers(now)
    }

    /// Determine the timeout to use in polling based on the current time
    /// (`now`), the `current` timeout and the next deadline in the shared
    /// timers.
    ///
    /// If there are no timers this will return `current`. If `current` is
    /// smaller than the next deadline in the timers this will also
    /// return `current`. Otherwise this will return a timeout based on the
    /// next deadline.
    pub(crate) fn next_timeout(&self, now: Instant, current: Option<Duration>) -> Option<Duration> {
        match self.timers.next() {
            Some(deadline) => match deadline.checked_duration_since(now) {
                // Timer has already expired, so no blocking.
                None => Some(Duration::ZERO),
                Some(timeout) => match current {
                    Some(current) if current < timeout => Some(current),
                    Some(..) | None => Some(timeout),
                },
            },
            None => current,
        }
    }

    /// Spawn a thread-safe actor.
    #[allow(clippy::needless_pass_by_value)] // For `ActorOptions`.
    pub(crate) fn try_spawn<S, NA>(
        self: &Arc<Self>,
        supervisor: S,
        new_actor: NA,
        arg: NA::Argument,
        options: ActorOptions,
    ) -> Result<ActorRef<NA::Message>, NA::Error>
    where
        S: Supervisor<NA> + Send + Sync + 'static,
        NA: NewActor<RuntimeAccess = ThreadSafe> + Sync + Send + 'static,
        NA::Actor: Send + Sync + 'static,
        NA::Message: Send,
    {
        self.scheduler.add_new_process(options.priority(), |pid| {
            let name = NA::name();
            debug!(pid = pid.0, name = name; "spawning thread-safe actor");
            let rt = ThreadSafe::new(pid, self.clone());
            ActorFuture::new(supervisor, new_actor, arg, rt)
        })
    }

    /// Spawn a thread-safe `future`.
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn spawn_future<Fut>(&self, future: Fut, options: FutureOptions)
    where
        Fut: Future<Output = ()> + Send + Sync + 'static,
    {
        _ = self.scheduler.add_new_process(options.priority(), |pid| {
            let process = FutureProcess(future);
            let name = process.name();
            debug!(pid = pid.0, name = name; "spawning thread-safe future");
            Ok::<_, !>((process, ()))
        });
    }

    /// See [`Scheduler::mark_ready`].
    pub(crate) fn mark_ready(&self, pid: ProcessId) {
        self.scheduler.mark_ready(pid);
    }

    /// Wake `n` worker threads.
    pub(crate) fn wake_workers(&self, n: usize) {
        trace!("waking {n} worker thread(s)");
        // To prevent the Thundering herd problem [1] we don't wake all workers,
        // only enough worker threads to handle all events. To spread the
        // workload (somewhat more) evenly we wake the workers in a Round-Robin
        // [2] fashion.
        //
        // [1]: https://en.wikipedia.org/wiki/Thundering_herd_problem
        // [2]: https://en.wikipedia.org/wiki/Round-robin_scheduling
        let n = min(n, self.worker_wakers.len());
        // Safety: needs to sync with itself.
        let wake_worker_idx =
            self.wake_worker_idx.fetch_add(n, Ordering::AcqRel) % self.worker_wakers.len();
        let (wake_second, wake_first) = self.worker_wakers.split_at(wake_worker_idx);
        let workers_to_wake = wake_first.iter().chain(wake_second.iter());
        let mut wakes_left = n;
        for worker in workers_to_wake {
            match worker.wake() {
                Ok(true) => {
                    wakes_left -= 1;
                    if wakes_left == 0 {
                        break;
                    }
                }
                Ok(false) => {}
                Err(err) => error!("error waking worker: {err}"),
            }
        }
    }

    /// Wake all worker threads, ignoring errors.
    pub(crate) fn wake_all_workers(&self) {
        trace!("waking all worker thread(s)");
        for worker in self.worker_wakers.iter() {
            drop(worker.wake());
        }
    }

    /// See [`Scheduler::has_process`].
    pub(crate) fn has_process(&self) -> bool {
        self.scheduler.has_process()
    }

    /// See [`Scheduler::has_ready_process`].
    pub(crate) fn has_ready_process(&self) -> bool {
        self.scheduler.has_ready_process()
    }

    /// See [`Scheduler::remove`].
    pub(crate) fn remove_process(&self) -> Option<Pin<Box<ProcessData>>> {
        self.scheduler.remove()
    }

    /// See [`Scheduler::add_back_process`].
    pub(crate) fn add_back_process(&self, process: Pin<Box<ProcessData>>) {
        self.scheduler.add_back_process(process);
    }

    /// See [`Scheduler::complete`].
    pub(crate) fn complete(&self, process: Pin<Box<ProcessData>>) {
        self.scheduler.complete(process);
    }

    pub(crate) fn start_trace(&self) -> Option<trace::EventTiming> {
        trace::start(&self.trace_log.as_deref())
    }

    pub(crate) fn finish_trace(
        &self,
        timing: Option<trace::EventTiming>,
        pid: ProcessId,
        description: &str,
        attributes: &[(&str, &dyn trace::AttributeValue)],
    ) {
        trace::finish(
            self.trace_log.as_deref(),
            timing,
            pid.0 as u64,
            description,
            attributes,
        );
    }
}
