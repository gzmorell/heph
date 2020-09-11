//! Functional tests.

#![feature(once_cell)]

use inbox::{new_small, Manager, Receiver, RecvError, SendError, Sender};

mod util;

use util::SMALL_CAP;

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}

#[test]
fn sender_is_send() {
    assert_send::<Sender<()>>();
}

#[test]
fn sender_is_sync() {
    assert_sync::<Sender<()>>();
}

#[test]
fn receiver_is_send() {
    assert_send::<Receiver<()>>();
}

#[test]
fn receiver_is_sync() {
    assert_sync::<Receiver<()>>();
}

#[test]
fn manager_is_send() {
    assert_send::<Manager<()>>();
}

#[test]
fn manager_is_sync() {
    assert_sync::<Manager<()>>();
}

#[test]
fn capacities_are_correct() {
    let (sender, receiver) = new_small::<()>();
    assert_eq!(sender.capacity(), SMALL_CAP);
    assert_eq!(receiver.capacity(), SMALL_CAP);
}

#[test]
fn sending_and_receiving_value() {
    let (mut sender, mut receiver) = new_small::<usize>();
    sender.try_send(123).unwrap();
    assert_eq!(receiver.try_recv().unwrap(), 123);
}

#[test]
fn receiving_from_empty_channel() {
    let (_sender, mut receiver) = new_small::<usize>();
    assert_eq!(receiver.try_recv().unwrap_err(), RecvError::Empty);
}

#[test]
fn receiving_from_disconnected_channel() {
    let (sender, mut receiver) = new_small::<usize>();
    drop(sender);
    assert_eq!(receiver.try_recv().unwrap_err(), RecvError::Disconnected);
}

#[test]
fn sending_into_full_channel() {
    let (mut sender, receiver) = new_small::<usize>();
    for value in 0..SMALL_CAP {
        sender.try_send(value).unwrap();
    }
    assert_eq!(
        sender.try_send(SMALL_CAP + 1),
        Err(SendError::Full(SMALL_CAP + 1))
    );
    drop(receiver);
}

#[test]
fn send_len_values_send_then_recv() {
    let (mut sender, mut receiver) = new_small::<usize>();
    for value in 0..SMALL_CAP {
        sender.try_send(value).unwrap();
    }
    assert!(sender.try_send(SMALL_CAP + 1).is_err());
    for value in 0..SMALL_CAP {
        assert_eq!(receiver.try_recv().unwrap(), value);
    }
    assert_eq!(receiver.try_recv().unwrap_err(), RecvError::Empty);
}

#[test]
fn send_len_values_interleaved() {
    let (mut sender, mut receiver) = new_small::<usize>();
    for value in 0..SMALL_CAP {
        sender.try_send(value).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), value);
    }
}

#[test]
fn send_2_len_values_send_then_recv() {
    let (mut sender, mut receiver) = new_small::<usize>();
    for value in 0..SMALL_CAP {
        sender.try_send(value).unwrap();
    }
    for value in 0..SMALL_CAP {
        assert_eq!(receiver.try_recv().unwrap(), value);
        sender.try_send(SMALL_CAP + value).unwrap();
    }
    for value in 0..SMALL_CAP {
        assert_eq!(receiver.try_recv().unwrap(), SMALL_CAP + value);
    }
}

#[test]
fn send_2_len_values_interleaved() {
    let (mut sender, mut receiver) = new_small::<usize>();
    for value in 0..2 * SMALL_CAP {
        sender.try_send(value).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), value);
    }
}

#[test]
fn sender_disconnected_after_send() {
    let (mut sender, mut receiver) = new_small::<usize>();
    sender.try_send(123).unwrap();
    drop(sender);
    assert_eq!(receiver.try_recv().unwrap(), 123);
    assert_eq!(receiver.try_recv().unwrap_err(), RecvError::Disconnected);
}

#[test]
fn sender_disconnected_after_send_len() {
    let (mut sender, mut receiver) = new_small::<usize>();
    for value in 0..SMALL_CAP {
        sender.try_send(value).unwrap();
    }
    drop(sender);
    for value in 0..SMALL_CAP {
        assert_eq!(receiver.try_recv().unwrap(), value);
    }
    assert_eq!(receiver.try_recv().unwrap_err(), RecvError::Disconnected);
}

#[test]
fn sender_disconnected_after_send_2_len() {
    let (mut sender, mut receiver) = new_small::<usize>();
    for value in 0..2 * SMALL_CAP {
        sender.try_send(value).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), value);
    }
    drop(sender);
    assert_eq!(receiver.try_recv().unwrap_err(), RecvError::Disconnected);
}

const LARGE: usize = 1_000_000;

#[test]
#[cfg_attr(not(feature = "stress_testing"), ignore)]
fn stress_sending_interleaved() {
    let (mut sender, mut receiver) = new_small::<usize>();
    for value in 0..LARGE {
        sender.try_send(value).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), value);
    }
    assert_eq!(receiver.try_recv(), Err(RecvError::Empty));
}

#[test]
#[cfg_attr(not(feature = "stress_testing"), ignore)]
fn stress_sending_fill() {
    for n in 1..=(SMALL_CAP - 1) {
        let (mut sender, mut receiver) = new_small::<usize>();

        for value in 0..(LARGE / n) {
            for n in 0..n {
                sender.try_send(value + n).unwrap();
            }
            for n in 0..n {
                assert_eq!(receiver.try_recv().unwrap(), value + n);
            }
        }

        assert_eq!(receiver.try_recv(), Err(RecvError::Empty));
    }
}

#[test]
fn sender_is_connected() {
    let (sender, receiver) = new_small::<usize>();
    assert!(sender.is_connected());
    drop(receiver);
    assert!(!sender.is_connected());
}

#[test]
fn receiver_is_connected() {
    let (sender, receiver) = new_small::<usize>();
    assert!(receiver.is_connected());
    drop(sender);
    assert!(!receiver.is_connected());
}

#[test]
fn same_channel() {
    let (sender1a, _) = new_small::<usize>();
    let sender1b = sender1a.clone();
    let (sender2a, _) = new_small::<usize>();
    let sender2b = sender2a.clone();

    assert!(sender1a.same_channel(&sender1a));
    assert!(sender1a.same_channel(&sender1b));
    assert!(!sender1a.same_channel(&sender2a));
    assert!(!sender1a.same_channel(&sender2b));
    assert!(sender1b.same_channel(&sender1a));
    assert!(sender1b.same_channel(&sender1b));
    assert!(!sender1b.same_channel(&sender2a));
    assert!(!sender1b.same_channel(&sender2b));

    assert!(!sender2a.same_channel(&sender1a));
    assert!(!sender2a.same_channel(&sender1b));
    assert!(sender2a.same_channel(&sender2a));
    assert!(sender2a.same_channel(&sender2b));
    assert!(!sender2b.same_channel(&sender1a));
    assert!(!sender2b.same_channel(&sender1b));
    assert!(sender2b.same_channel(&sender2a));
    assert!(sender2b.same_channel(&sender2b));
}

#[test]
fn sends_to() {
    let (sender1a, receiver1) = new_small::<usize>();
    let sender1b = sender1a.clone();
    let (sender2a, receiver2) = new_small::<usize>();
    let sender2b = sender2a.clone();

    assert!(sender1a.sends_to(&receiver1));
    assert!(!sender1a.sends_to(&receiver2));
    assert!(sender1b.sends_to(&receiver1));
    assert!(!sender1b.sends_to(&receiver2));

    assert!(!sender2a.sends_to(&receiver1));
    assert!(sender2a.sends_to(&receiver2));
    assert!(!sender2b.sends_to(&receiver1));
    assert!(sender2b.sends_to(&receiver2));
}

#[test]
fn receiver_new_sender() {
    let (sender, mut receiver) = new_small::<usize>();

    let mut sender2 = receiver.new_sender();
    assert!(sender2.sends_to(&receiver));
    assert!(sender2.same_channel(&sender));

    drop(sender);
    assert!(sender2.is_connected());
    assert!(receiver.is_connected());

    sender2.try_send(123).unwrap();
    assert_eq!(receiver.try_recv().unwrap(), 123);

    drop(receiver);
    assert!(!sender2.is_connected());
}

mod future {
    //! Tests for the `Future` implementations.

    use std::cmp::min;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{self, Poll};

    use futures_test::task::{new_count_waker, AwokenCount};

    use inbox::{new_small, Sender};

    use super::SMALL_CAP;

    macro_rules! pin_stack {
        ($fut: ident) => {
            let mut $fut = $fut;
            #[allow(unused_mut)]
            let mut $fut = unsafe { Pin::new_unchecked(&mut $fut) };
        };
    }

    #[test]
    fn send_value() {
        let (mut sender, mut receiver) = new_small::<usize>();

        let (waker, count) = new_count_waker();
        let mut ctx = task::Context::from_waker(&waker);

        let future = sender.send(10);
        pin_stack!(future);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(Ok(())));
        assert_eq!(count.get(), 0);
        assert_eq!(receiver.try_recv(), Ok(10));
    }

    #[test]
    fn send_value_full_channel() {
        let (mut sender, mut receiver) = new_small::<usize>();
        // Fill the channel.
        for value in 0..SMALL_CAP {
            sender.try_send(value).unwrap();
        }

        let (waker, count) = new_count_waker();
        let mut ctx = task::Context::from_waker(&waker);

        let future = sender.send(SMALL_CAP);
        pin_stack!(future);

        // Channel should be full.
        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Pending);
        assert_eq!(count.get(), 0);

        // Receiving a value should wake a sender.
        assert_eq!(receiver.try_recv(), Ok(0));
        assert_eq!(count.get(), 1);
        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(Ok(())));

        for want in 1..SMALL_CAP + 1 {
            assert_eq!(receiver.try_recv(), Ok(want));
        }
    }

    #[test]
    fn send_many_values() {
        let (mut sender, mut receiver) = new_small::<usize>();

        let (waker, count) = new_count_waker();
        let mut ctx = task::Context::from_waker(&waker);

        for value in 0..SMALL_CAP {
            let future = sender.send(value);
            pin_stack!(future);

            assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(Ok(())));
            assert_eq!(count.get(), 0);
        }

        for value in 0..SMALL_CAP {
            assert_eq!(receiver.try_recv(), Ok(value));
        }
    }

    #[test]
    fn send_many_values_interleaved() {
        let (mut sender, mut receiver) = new_small::<usize>();

        let (waker, count) = new_count_waker();
        let mut ctx = task::Context::from_waker(&waker);

        for value in 0..SMALL_CAP {
            let future = sender.send(value);
            pin_stack!(future);

            assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(Ok(())));
            assert_eq!(count.get(), 0);

            assert_eq!(receiver.try_recv(), Ok(value));
        }
    }

    // Test where `n` sender try to send into a full channel.
    fn send_many_values_full_channel_test(n: usize) {
        let (mut sender, mut receiver) = new_small::<usize>();
        // Fill the channel.
        for value in 0..SMALL_CAP {
            sender.try_send(value).unwrap();
        }

        // Create a `Sender` for each `SendValue` future.
        let mut senders = (0..n)
            .map(|_| sender.clone())
            .collect::<Vec<Sender<usize>>>();
        let mut senders = &mut *senders;

        // Create a number of `SendValue` futures.
        let mut futures: Vec<(task::Waker, AwokenCount, _)> = Vec::with_capacity(n);
        for index in 0..n {
            let (waker, count) = new_count_waker();
            let mut ctx = task::Context::from_waker(&waker);

            // Work around borrow rules: ensure that we only access a single
            // sender in the vector at a time.
            let (head, tail) = senders.split_first_mut().unwrap();
            senders = tail;
            let mut future = Box::pin(head.send(index + SMALL_CAP));

            assert_eq!(future.as_mut().poll(&mut ctx), Poll::Pending);
            assert_eq!(count.get(), 0);

            futures.push((waker, count, future));
        }

        for value in 0..SMALL_CAP {
            // Receiving a value should wake the correct future.
            assert_eq!(receiver.try_recv(), Ok(value));
            if value < n {
                assert_eq!(futures[value].1.get(), 1);
            }
        }

        for (waker, count, mut future) in futures.drain(..min(SMALL_CAP, futures.len())) {
            assert_eq!(count.get(), 1);

            let mut ctx = task::Context::from_waker(&waker);
            assert_eq!(Pin::new(&mut future).poll(&mut ctx), Poll::Ready(Ok(())));
            assert_eq!(count.get(), 1);
        }

        while !futures.is_empty() {
            for value in (0..futures.len()).take(SMALL_CAP) {
                assert_eq!(receiver.try_recv(), Ok(value + SMALL_CAP));
            }

            for (waker, count, mut future) in futures.drain(..min(SMALL_CAP, futures.len())) {
                assert_eq!(count.get(), 1);

                let mut ctx = task::Context::from_waker(&waker);
                assert_eq!(Pin::new(&mut future).poll(&mut ctx), Poll::Ready(Ok(())));
                assert_eq!(count.get(), 1);
            }
        }
    }

    #[test]
    fn send_many_values_full_channel_one_sender() {
        send_many_values_full_channel_test(1);
    }

    #[test]
    fn send_many_values_full_channel_two_senders() {
        send_many_values_full_channel_test(2);
    }

    #[test]
    fn send_many_values_full_channel_three_senders() {
        send_many_values_full_channel_test(3);
    }

    #[test]
    fn send_many_values_full_channel_four_senders() {
        send_many_values_full_channel_test(4);
    }

    #[test]
    fn send_many_values_full_channel_len_senders() {
        send_many_values_full_channel_test(SMALL_CAP);
    }

    #[test]
    fn send_many_values_full_channel_many_senders() {
        send_many_values_full_channel_test(2 * SMALL_CAP);
    }

    #[test]
    fn send_value_supports_polling_with_different_wakers() {
        let (mut sender, mut receiver) = new_small::<usize>();

        for _ in 0..SMALL_CAP {
            sender.try_send(123).unwrap();
        }

        let (waker1, count1) = new_count_waker();
        let (waker2, count2) = new_count_waker();
        let mut ctx1 = task::Context::from_waker(&waker1);
        let mut ctx2 = task::Context::from_waker(&waker2);

        let mut future = Box::pin(sender.send(10));
        assert_eq!(future.as_mut().poll(&mut ctx1), Poll::Pending);
        assert_eq!(future.as_mut().poll(&mut ctx2), Poll::Pending);

        for _ in 0..SMALL_CAP {
            assert_eq!(receiver.try_recv().unwrap(), 123);
        }
        drop(receiver);

        assert_eq!(count1.get(), 0);
        assert_eq!(count2.get(), 1);
    }

    #[test]
    fn recv_value() {
        let (waker, count) = new_count_waker();
        let (mut sender, mut receiver) = new_small::<usize>();

        let mut ctx = task::Context::from_waker(&waker);

        let future = receiver.recv();
        pin_stack!(future);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Pending);

        sender.try_send(10).unwrap();
        assert_eq!(count.get(), 1);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(Some(10)));
    }

    #[test]
    fn recv_value_wake_up_optimised() {
        let (waker, count) = new_count_waker();
        let (mut sender, mut receiver) = new_small::<usize>();

        sender.try_send(10).unwrap();
        assert_eq!(count.get(), 0); // Wake-up optimised away.

        let mut ctx = task::Context::from_waker(&waker);

        let future = receiver.recv();
        pin_stack!(future);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(Some(10)));
    }

    #[test]
    fn recv_value_empty() {
        let (waker, count) = new_count_waker();

        let (mut sender, mut receiver) = new_small::<usize>();

        let mut ctx = task::Context::from_waker(&waker);

        let future = receiver.recv();
        pin_stack!(future);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Pending);
        assert_eq!(count.get(), 0);

        sender.try_send(10).unwrap();

        assert_eq!(count.get(), 1);
        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(Some(10)));
    }

    #[test]
    fn recv_value_all_senders_disconnected() {
        let (waker, count) = new_count_waker();

        let (sender, mut receiver) = new_small::<usize>();

        let mut ctx = task::Context::from_waker(&waker);

        let future = receiver.recv();
        pin_stack!(future);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Pending);

        // Dropping the last sender should notify the receiver.
        drop(sender);
        assert_eq!(count.get(), 1);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(None));
    }

    #[test]
    fn recv_value_all_senders_disconnected_not_empty() {
        let (waker, count) = new_count_waker();

        let (mut sender, mut receiver) = new_small::<usize>();

        let mut ctx = task::Context::from_waker(&waker);

        let future = receiver.recv();
        pin_stack!(future);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Pending);

        // Sending and dropping the last sender should wake the receiver.
        sender.try_send(10).unwrap();
        assert_eq!(count.get(), 1);
        drop(sender);
        assert_eq!(count.get(), 1); // Wake-up optimised away.

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(Some(10)));
        let mut future = receiver.recv();
        assert_eq!(Pin::new(&mut future).poll(&mut ctx), Poll::Ready(None));
    }

    #[test]
    fn recv_value_all_senders_disconnected_cloned_sender() {
        let (waker, count) = new_count_waker();

        let (sender, mut receiver) = new_small::<usize>();
        let sender2 = sender.clone();

        let mut ctx = task::Context::from_waker(&waker);

        let future = receiver.recv();
        pin_stack!(future);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Pending);

        // Only dropping the last sender should wake the receiver.
        drop(sender);
        assert_eq!(count.get(), 0);
        drop(sender2);
        assert_eq!(count.get(), 1);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(None));
    }

    #[test]
    fn recv_value_only_wake_if_polled() {
        let (waker, count) = new_count_waker();

        let (sender, mut receiver) = new_small::<usize>();

        let mut ctx = task::Context::from_waker(&waker);

        let future = receiver.recv();
        pin_stack!(future);

        drop(sender);
        // `RecvValue` isn't polled yet, so we shouldn't receive a wake-up
        // notification.
        assert_eq!(count.get(), 0);

        assert_eq!(future.as_mut().poll(&mut ctx), Poll::Ready(None));
    }
}

mod manager {
    use inbox::{Manager, ReceiverConnected};

    #[test]
    fn new_sender() {
        let (manager, mut sender1, mut receiver) = Manager::<usize>::new_small_channel();
        let mut sender2 = manager.new_sender();

        sender1.try_send(123).unwrap();
        sender2.try_send(456).unwrap();

        assert_eq!(receiver.try_recv().unwrap(), 123);
        assert_eq!(receiver.try_recv().unwrap(), 456);
    }

    #[test]
    fn new_receiver() {
        let (manager, mut sender, receiver) = Manager::<usize>::new_small_channel();
        sender.try_send(123).unwrap();

        drop(receiver);
        sender.try_send(456).unwrap();

        let mut receiver = manager.new_receiver().unwrap();

        assert_eq!(receiver.try_recv().unwrap(), 123);
        assert_eq!(receiver.try_recv().unwrap(), 456);
    }

    #[test]
    fn new_receiver_already_exists() {
        let (manager, _sender, _receiver) = Manager::<usize>::new_small_channel();
        assert_eq!(manager.new_receiver().unwrap_err(), ReceiverConnected);
    }

    #[test]
    fn sending_and_receiving_value() {
        let (manager, mut sender, mut receiver) = Manager::<usize>::new_small_channel();
        sender.try_send(123).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), 123);
        drop(manager);
    }

    #[test]
    fn sender_is_connected() {
        let (manager, sender, receiver) = Manager::<usize>::new_small_channel();
        assert!(sender.is_connected());
        drop(receiver);
        // Manager is still alive.
        assert!(sender.is_connected());
        drop(manager);
        assert!(!sender.is_connected());
    }

    #[test]
    fn receiver_is_connected() {
        let (manager, sender, receiver) = Manager::<usize>::new_small_channel();
        assert!(receiver.is_connected());
        drop(manager);
        assert!(receiver.is_connected());
        drop(sender);
        assert!(!receiver.is_connected());

        let (manager, sender, receiver) = Manager::<usize>::new_small_channel();
        assert!(receiver.is_connected());
        drop(sender);
        assert!(!receiver.is_connected());
        let new_sender = manager.new_sender();
        assert!(receiver.is_connected());
        drop(new_sender);
        assert!(!receiver.is_connected());
    }

    #[test]
    fn same_channel() {
        let (manager1, sender1a, _) = Manager::<usize>::new_small_channel();
        let sender1b = manager1.new_sender();
        let (manager2, sender2a, _) = Manager::<usize>::new_small_channel();
        let sender2b = manager2.new_sender();

        assert!(sender1a.same_channel(&sender1a));
        assert!(sender1a.same_channel(&sender1b));
        assert!(!sender1a.same_channel(&sender2a));
        assert!(!sender1a.same_channel(&sender2b));
        assert!(sender1b.same_channel(&sender1a));
        assert!(sender1b.same_channel(&sender1b));
        assert!(!sender1b.same_channel(&sender2a));
        assert!(!sender1b.same_channel(&sender2b));

        assert!(!sender2a.same_channel(&sender1a));
        assert!(!sender2a.same_channel(&sender1b));
        assert!(sender2a.same_channel(&sender2a));
        assert!(sender2a.same_channel(&sender2b));
        assert!(!sender2b.same_channel(&sender1a));
        assert!(!sender2b.same_channel(&sender1b));
        assert!(sender2b.same_channel(&sender2a));
        assert!(sender2b.same_channel(&sender2b));
    }

    #[test]
    fn sends_to() {
        let (manager1, sender1a, receiver1) = Manager::<usize>::new_small_channel();
        let sender1b = manager1.new_sender();
        let (manager2, sender2a, receiver2) = Manager::<usize>::new_small_channel();
        let sender2b = manager2.new_sender();

        assert!(sender1a.sends_to(&receiver1));
        assert!(!sender1a.sends_to(&receiver2));
        assert!(sender1b.sends_to(&receiver1));
        assert!(!sender1b.sends_to(&receiver2));

        assert!(!sender2a.sends_to(&receiver1));
        assert!(sender2a.sends_to(&receiver2));
        assert!(!sender2b.sends_to(&receiver1));
        assert!(sender2b.sends_to(&receiver2));
    }
}
