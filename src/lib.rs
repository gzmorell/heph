//! Heph derived from Hephaestus, is the Greek god of blacksmiths, metalworking,
//! carpenters, craftsmen, artisans, sculptors, metallurgy, fire, and volcanoes.
//! <sup>[1]</sup> Well this crate has very little to do with Greek gods, but I
//! needed a name.
//!
//!
//! ## About
//!
//! Heph is an actor <sup>[2]</sup> framework based on asynchronous functions.
//! Such an asynchronous function looks like this:
//!
//! ```
//! # #![feature(async_await, never_type)]
//! #
//! # use heph::actor;
//! #
//! async fn actor(mut ctx: actor::Context<String>) -> Result<(), !> {
//!     // Receive a message.
//!     let msg = ctx.receive_next().await;
//!     // Print the message.
//!     println!("got a message: {}", msg);
//!     // And we're done.
//!     Ok(())
//! }
//!
//! # drop(actor); // Silence dead code warnings.
//! ```
//!
//! Heph uses an event-driven, non-blocking I/O, share nothing design. But what
//! do all those buzzwords actually mean?
//!
//!  - *Event-driven*: Heph does nothing by itself, it must first get an event
//!    before it starts doing anything. For example when using an `TcpListener`
//!    it waits on a notification from the OS saying the `TcpListener` is ready
//!    before trying to accept connections.
//!  - *Non-blocking I/O*: normal I/O operations need to wait (block) until the
//!    operation can complete. Using non-blocking, or asynchronous, I/O means
//!    that rather then waiting for the operation to complete we'll do some
//!    other, more useful, work and try the operation later.
//!  - *Share nothing*: a lot of application share data across multiple threads.
//!    To do this safely we need to protect it from data races, via a [`Mutex`]
//!    or by using [atomic] operations. Heph is designed to not share any data.
//!    Each actor is responsible for its own memory and cannot access memory
//!    owned by other actors. Instead communication is done via sending
//!    messages, see the [actor model].
//!
//! [`Mutex`]: std::sync::Mutex
//! [atomic]: https://doc.rust-lang.org/std/sync/atomic/index.html
//! [actor model]: https://en.wikipedia.org/wiki/Actor_model
//!
//!
//! ## Getting started
//!
//! The easiest way to get start with Heph is looking at the examples in the
//! examples directory of the source code. Or by looking through the API
//! documentation, starting with [`ActorSystem`].
//!
//!
//! ## Features
//!
//! This crate has a single optional feature: `test`. This feature will enable
//! the `test` module which adds testing facilities.
//!
//! [1]: https://en.wikipedia.org/wiki/Hephaestus
//! [2]: https://en.wikipedia.org/wiki/Actor_model

#![feature(
    async_await,
    const_fn,
    never_type,
    non_exhaustive,
    read_initializer,
    weak_ptr_eq
)]
#![cfg_attr(test, feature(const_slice_len))]
#![warn(
    anonymous_parameters,
    bare_trait_objects,
    missing_debug_implementations,
    missing_docs,
    rust_2018_idioms,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results,
    variant_size_differences
)]
// Disallow warnings when running tests.
#![cfg_attr(test, deny(warnings))]
// Disallow warnings in examples, we want to set a good example after all.
#![doc(test(attr(deny(warnings))))]

pub mod actor;
pub mod actor_ref;
pub mod log;
pub mod net;
pub mod supervisor;
pub mod system;
pub mod timer;

#[cfg(any(test, feature = "test"))]
pub mod test;

mod inbox;
mod util;

#[doc(no_inline)]
pub use crate::actor::{Actor, NewActor};
#[doc(no_inline)]
pub use crate::actor_ref::ActorRef;
#[doc(no_inline)]
pub use crate::supervisor::{Supervisor, SupervisorStrategy};
#[doc(no_inline)]
pub use crate::system::{ActorOptions, ActorSystem, ActorSystemRef};
