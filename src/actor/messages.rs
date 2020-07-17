//! Module with some generic messages.
//!
//! To use these message the receiving actor should implement [`From`]`<Message>`,
//! this way the sending actor can simply send the message, without having to
//! wrap it in a message type first. See the examples below.
//!
//! Most message types have an optional id, defaulting to `()`. This allows a
//! single actor to receive messages from multiple sources with the ability to
//! differentiate the source of the message.
//!
//! Three more default messages should be considered, the two variants from
//! [`Result`]: [`Ok`] and [`Err`] and [`Signal`] to handle process signals.
//!
//! [`Signal`]: crate::rt::Signal
//!
//! # Examples
//!
//! Implementing `From<Message>` to allow for easy sending of messages.
//!
//! ```
//! #![feature(never_type)]
//!
//! use heph::actor::messages::Ack;
//!
//! #[derive(Debug, Eq, PartialEq)]
//! struct OK;
//!
//! #[derive(Debug, Eq, PartialEq)]
//! struct Error;
//!
//! /// The message type for the coordinating actor.
//! #[derive(Debug, Eq, PartialEq)]
//! enum Message {
//!     /// Acknowledgement of receiving an message.
//!     Ack(usize),
//!     /// An ok result.
//!     Ok(OK),
//!     /// An erroneous result.
//!     Error(Error),
//! }
//!
//! // This allows us to receive an `Ack` message.
//! impl From<Ack<usize>> for Message {
//!     fn from(ack: Ack<usize>) -> Message {
//!         Message::Ack(ack.0)
//!     }
//! }
//!
//! // Abilities to receive an result from a working actor.
//! impl From<Result<OK, Error>> for Message {
//!     fn from(res: Result<OK, Error>) -> Message {
//!         match res {
//!             Ok(ok) => Message::Ok(ok),
//!             Err(err) => Message::Error(err),
//!         }
//!     }
//! }
//! #
//! # drop(Message::Ack(0));
//! ```

use serde::{Deserialize, Serialize};

/// A start signal
///
/// Useful for example when you want to delay the start of an actor. This
/// message has an optional id.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct Start<Id = ()>(pub Id);

/// An acknowledgement.
///
/// Useful for example when you want to know if a message was received. This
/// message has an optional id.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct Ack<Id = ()>(pub Id);

/// Signal to an actor that we're done.
///
/// This message has an optional id.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct Done<Id = ()>(pub Id);

/// Signal to an actor to cancel an operation.
///
/// This message has an optional id.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct Cancel<Id = ()>(pub Id);

/// Ask an actor to terminate.
///
/// # Notes
///
/// This message is not special in anyway, this means the receiving actor can
/// simply ignore this message and continue living.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct Terminate;
