//! TODO: docs

use std::io;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use mio_st::event::{Events, Evented, EventedId, Ready};
use mio_st::poll::{Poll, PollOpt};

use actor::Actor;
use initiator::Initiator;

mod actor_process;
mod initiator_process;
mod builder;
mod scheduler;

pub mod error;
pub mod options;

pub use self::actor_process::ActorRef;
pub use self::builder::ActorSystemBuilder;
pub use self::options::{ActorOptions, InitiatorOptions};
pub(crate) use self::scheduler::ProcessId;

use self::actor_process::ActorProcess;
use self::error::{AddActorError, AddActorErrorReason, AddInitiatorError, AddInitiatorErrorReason, RuntimeError, ERR_SYSTEM_SHUTDOWN};
use self::initiator_process::InitiatorProcess;
use self::scheduler::{Scheduler, Priority};

/// The system that runs all actors.
#[derive(Debug)]
pub struct ActorSystem {
    /// Inside of the system, shared (via weak references) with
    /// `ActorSystemRef`s.
    inner: Rc<RefCell<ActorSystemInner>>,
}

impl ActorSystem {
    /// Add a new actor to the system.
    // TODO: remove `'static` lifetime.
    pub fn add_actor<A>(&mut self, actor: A, options: ActorOptions) -> Result<ActorRef<A>, AddActorError<A>>
        where A: Actor + 'static,
    {
        self.inner.borrow_mut().add_actor(actor, options)
    }

    /// Add a new initiator to the system.
    // TODO: remove `'static` lifetime.
    pub fn add_initiator<I>(&mut self, initiator: I, options: InitiatorOptions) -> Result<(), AddInitiatorError<I>>
        where I: Initiator + 'static,
    {
        self.inner.borrow_mut().add_initiator(initiator, options)
    }

    /// Create a new reference to this actor system.
    pub fn create_ref(&self) -> ActorSystemRef {
        ActorSystemRef {
            inner: Rc::downgrade(&self.inner),
        }
    }

    /// Run the actor system.
    pub fn run(self) -> Result<(), RuntimeError> {
        let mut system_ref = self.create_ref();
        debug!("running actor system");

        loop {
            debug!("polling system poll for events");
            let n_events = self.inner.borrow_mut().poll()
                .map_err(RuntimeError::Poll)?;

            // Allow the system to be run without any initiators. In that case
            // we will only handle user space events (e.g. sending messages) and
            // will return after those are all handled.
            if !self.inner.borrow().has_initiators && n_events == 0 {
                debug!("no events, no initiators stopping actor system");
                return Ok(())
            }

            // Run all scheduled processes.
            self.inner.borrow_mut().scheduler.run(&mut system_ref);
        }
    }
}

/// A reference to an [`ActorSystem`].
///
/// This reference can be shared by cloning it, a very cheap operation, just
/// like [`ActorRef`].
///
/// [`ActorSystem`]: struct.ActorSystem.html
/// [`ActorRef`]: struct.ActorRef.html
#[derive(Debug)]
pub struct ActorSystemRef {
    /// A non-owning reference to the actor system internals.
    inner: Weak<RefCell<ActorSystemInner>>,
}

impl ActorSystemRef {
    /// Add a new actor to the system.
    ///
    /// See [`ActorSystem.add_actor`].
    ///
    /// [`ActorSystem.add_actor`]: struct.ActorSystem.html#method.add_actor
    // TODO: keep this in sync with `ActorSystemRef.add_actor`.
    // TODO: remove `'static` lifetime,
    pub fn add_actor<A>(&mut self, actor: A, options: ActorOptions) -> Result<ActorRef<A>, AddActorError<A>>
        where A: Actor + 'static,
    {
        match self.inner.upgrade() {
            Some(r) => r.borrow_mut().add_actor(actor, options),
            None => Err(AddActorError::new(actor, AddActorErrorReason::SystemShutdown)),
        }
    }

    /// Register an `Evented` handle, see `Poll.register`.
    pub(crate) fn poll_register<E>(&mut self, handle: &mut E, id: EventedId, interests: Ready, opt: PollOpt) -> io::Result<()>
        where E: Evented + ?Sized
    {
        match self.inner.upgrade() {
            Some(r) => r.borrow_mut().poll.register(handle, id, interests, opt),
            None => Err(io::Error::new(io::ErrorKind::Other, ERR_SYSTEM_SHUTDOWN)),
        }
    }

    /// Deregister an `Evented` handle, see `Poll.deregister`.
    pub(crate) fn poll_deregister<E>(&mut self, handle: &mut E) -> io::Result<()>
    where
        E: Evented + ?Sized,
    {
        match self.inner.upgrade() {
            Some(r) => r.borrow_mut().poll.deregister(handle),
            None => Err(io::Error::new(io::ErrorKind::Other, ERR_SYSTEM_SHUTDOWN)),
        }
    }
}

impl Clone for ActorSystemRef {
    fn clone(&self) -> ActorSystemRef {
        ActorSystemRef {
            inner: Weak::clone(&self.inner),
        }
    }
}

/// Inside of the `ActorSystem`, to which `ActorSystemRef`s have a reference to.
#[derive(Debug)]
struct ActorSystemInner {
    /// Scheduler that hold the processes, schedules and runs them.
    scheduler: Scheduler,
    /// Whether or not the system has initiators, this is used to allow the
    /// system to run without them. Otherwise we would poll with no timeout,
    /// waiting for ever.
    has_initiators: bool,
    /// System poller, used for event notifications to support non-block I/O.
    poll: Poll,
}

impl ActorSystemInner {
    fn add_actor<A>(&mut self, actor: A, options: ActorOptions) -> Result<ActorRef<A>, AddActorError<A>>
        where A: Actor + 'static,
    {
        // Setup adding a new process to the scheduler.
        let process_entry = self.scheduler.add_process();
        let pid = process_entry.id();
        debug!("adding actor to actor system: pid={}", pid);

        // Create a new actor process.
        let priority = options.priority;
        let process = ActorProcess::new(pid, actor, options, &mut self.poll)
            .map_err(|(actor, err)| AddActorError::new(actor, AddActorErrorReason::RegisterFailed(err)))?;

        // Create a reference to the actor, to be returned.
        let actor_ref = process.create_ref();

        // Actually add the process.
        process_entry.add(process, priority);
        Ok(actor_ref)
    }

    fn add_initiator<I>(&mut self, mut initiator: I, _options: InitiatorOptions) -> Result<(), AddInitiatorError<I>>
        where I: Initiator + 'static,
    {
        // Setup adding a new process to the scheduler.
        let process_entry = self.scheduler.add_process();
        let pid = process_entry.id();
        debug!("adding initiator to actor system: pid={}", pid);

        // Initialise the initiator.
        if let Err(err) = initiator.init(&mut self.poll, pid) {
            return Err(AddInitiatorError {
                initiator,
                reason: AddInitiatorErrorReason::InitFailed(err),
            });
        }

        // Create a new initiator process.
        let process = InitiatorProcess::new(initiator);

        // Actually add the process.
        // Initiators will always have a low priority this way requests in
        // progress are first handled before new requests are accepted and
        // possibly overload the system.
        process_entry.add(process, Priority::LOW);
        self.has_initiators = true;
        Ok(())
    }

    /// Poll the system poll and schedule the notified processes, returns the
    /// number of processes scheduled.
    fn poll(&mut self) -> io::Result<usize> {
        // In case of no initiators only user space events are handled and the
        // system is stopped otherwise.
        let timeout = if self.has_initiators {
            None
        } else {
            Some(Duration::from_millis(0))
        };

        let mut events = Events::new();
        self.poll.poll(&mut events, timeout)?;

        // Schedule any processes that we're notified off.
        let n_scheduled = events.len();
        for event in &mut events {
            let pid = event.id().into();
            self.scheduler.schedule(pid);
        }

        Ok(n_scheduled)
    }
}
