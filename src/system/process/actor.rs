//! Module containing the implementation of the `Process` trait for `Actor`s.

use std::pin::Pin;
use std::task::{self, Poll};

use log::error;

use crate::inbox::Inbox;
use crate::supervisor::SupervisorStrategy;
use crate::system::process::{Process, ProcessId, ProcessResult};
use crate::{actor, Actor, ActorSystemRef, NewActor, Supervisor};

/// A process that represent an [`Actor`].
pub struct ActorProcess<S, NA: NewActor> {
    supervisor: S,
    new_actor: NA,
    actor: NA::Actor,
    /// The inbox of the actor, used in create a new `actor::Context` if the
    /// actor is restarted.
    inbox: Inbox<NA::Message>,
}

impl<S, NA: NewActor> ActorProcess<S, NA> {
    /// Create a new `ActorProcess`.
    pub(crate) const fn new(
        supervisor: S,
        new_actor: NA,
        actor: NA::Actor,
        inbox: Inbox<NA::Message>,
    ) -> ActorProcess<S, NA> {
        ActorProcess {
            supervisor,
            new_actor,
            actor,
            inbox,
        }
    }
}

impl<S, NA> Process for ActorProcess<S, NA>
where
    S: Supervisor<<NA::Actor as Actor>::Error, NA::Argument>,
    NA: NewActor + 'static,
{
    fn run(self: Pin<&mut Self>, system_ref: &mut ActorSystemRef, pid: ProcessId) -> ProcessResult {
        // This is safe because we're not moving any values.
        let this = unsafe { Pin::get_unchecked_mut(self) };

        // The actor need to be called with `Pin`. So we're undoing the previous
        // operation, still ensuring that the actor is not moved.
        let mut pinned_actor = unsafe { Pin::new_unchecked(&mut this.actor) };
        let waker = system_ref.new_waker(pid);
        let mut ctx = task::Context::from_waker(&waker);
        match Actor::try_poll(pinned_actor.as_mut(), &mut ctx) {
            Poll::Ready(Ok(())) => ProcessResult::Complete,
            Poll::Ready(Err(err)) => match this.supervisor.decide(err) {
                SupervisorStrategy::Restart(arg) => {
                    // Create a new actor.
                    let ctx = actor::Context::new(pid, system_ref.clone(), this.inbox.clone());
                    match this.new_actor.new(ctx, arg) {
                        Ok(actor) => {
                            pinned_actor.set(actor);
                            // Run the actor, just in case progress can be made
                            // already.
                            unsafe { Pin::new_unchecked(this) }.run(system_ref, pid)
                        }
                        Err(err) => {
                            // New actor can't be created, so all we can do
                            // is log and mark the process as complete.
                            error!("error creating new actor: {}", err);
                            ProcessResult::Complete
                        }
                    }
                }
                SupervisorStrategy::Stop => ProcessResult::Complete,
            },
            Poll::Pending => ProcessResult::Pending,
        }
    }
}
