//! Module containing the implementation of the `Process` trait for `Actor`s.

use std::fmt;
use std::mem::{drop, replace};
use std::pin::Pin;
use std::task::{LocalWaker, Poll};

use log::trace;

use crate::actor::{Actor, NewActor, ActorContext};
use crate::mailbox::MailBox;
use crate::process::{Process, ProcessId, ProcessResult};
use crate::supervisor::{Supervisor, SupervisorStrategy};
use crate::system::ActorSystemRef;
use crate::util::Shared;

/// A process that represent an [`Actor`].
///
/// [`Actor`]: ../../actor/trait.Actor.html
pub struct ActorProcess<S, NA: NewActor, A> {
    /// The id of this process.
    pid: ProcessId,
    /// Supervisor of the actor.
    supervisor: S,
    /// `NewActor` used to restart the actor.
    new_actor: NA,
    /// The actor.
    actor: A,
    /// The inbox of the actor, used in create a new `ActorContext` if the actor
    /// is restarted.
    inbox: Shared<MailBox<NA::Message>>,
    /// Waker used in the futures context.
    waker: LocalWaker,
}

impl<S, NA: NewActor, A> ActorProcess<S, NA, A> {
    /// Create a new `ActorProcess`.
    pub(crate) fn new(pid: ProcessId, supervisor: S, new_actor: NA, actor: A, inbox: Shared<MailBox<NA::Message>>, waker: LocalWaker) -> ActorProcess<S, NA, A> {
        ActorProcess {
            pid,
            supervisor,
            new_actor,
            inbox,
            actor,
            waker,
        }
    }
}

impl<S, NA, A> Process for ActorProcess<S, NA, A>
    where S: Supervisor<A::Error, NA::Argument>,
          A: Actor,
          NA: NewActor<Actor = A>,
{
    fn run(&mut self, system_ref: &mut ActorSystemRef) -> ProcessResult {
        trace!("running actor process");

        // FIXME: Currently this is safe because `ProcessData` in the scheduler
        // module boxes each process, but this needs improvement. Maybe go the
        // future route: `self: PinMut<Self>`.
        let actor = unsafe { Pin::new_unchecked(&mut self.actor) };

        match actor.try_poll(&self.waker) {
            Poll::Ready(Ok(())) => ProcessResult::Complete,
            Poll::Ready(Err(err)) => {
                match self.supervisor.decide(err) {
                    SupervisorStrategy::Restart(arg) => {
                        // Create a new actor.
                        let ctx = ActorContext::new(self.pid, system_ref.clone(), self.inbox.clone());
                        let actor = self.new_actor.new(ctx, arg);
                        drop(replace(&mut self.actor, actor));
                        // Run the actor, just in case progress can be made
                        // already.
                        return self.run(system_ref);
                    },
                    SupervisorStrategy::Stop => ProcessResult::Complete,
                }
            },
            Poll::Pending => ProcessResult::Pending,
        }
    }
}

impl<S, NA: NewActor, A> fmt::Debug for ActorProcess<S, NA, A> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ActorProcess")
            .finish()
    }
}
