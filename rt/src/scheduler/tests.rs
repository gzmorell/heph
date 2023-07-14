//! Tests for the local scheduler.

use std::cell::RefCell;
use std::future::pending;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{self, Poll};

use heph::actor::{self, actor_fn, ActorFuture};
use heph::supervisor::NoSupervisor;

use crate::process::{FutureProcess, Process, ProcessId};
use crate::scheduler::{ProcessData, Scheduler};
use crate::spawn::options::Priority;
use crate::test::{self, assert_size, nop_task_waker, AssertUnmoved, TestAssertUnmovedNewActor};
use crate::ThreadLocal;

#[test]
fn size_assertions() {
    assert_size::<ProcessData>(40);
}

#[derive(Debug)]
struct NopTestProcess;

impl Future for NopTestProcess {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut task::Context<'_>) -> Poll<()> {
        unimplemented!();
    }
}

impl Process for NopTestProcess {
    fn name(&self) -> &'static str {
        "NopTestProcess"
    }
}

#[test]
fn has_process() {
    let mut scheduler = Scheduler::new();
    assert!(!scheduler.has_process());
    assert!(!scheduler.has_ready_process());

    let _ = scheduler.add_new_process(Priority::NORMAL, FutureProcess(NopTestProcess));
    assert!(scheduler.has_process());
    assert!(scheduler.has_ready_process());
}

async fn simple_actor(_: actor::Context<!, ThreadLocal>) {}

#[test]
fn add_actor() {
    let mut scheduler = Scheduler::new();
    let new_actor = actor_fn(simple_actor);
    let rt = ThreadLocal::new(test::runtime());
    let (process, _) = ActorFuture::new(NoSupervisor, new_actor, (), rt).unwrap();
    let _ = scheduler.add_new_process(Priority::NORMAL, process);
    assert!(scheduler.has_process());
    assert!(scheduler.has_ready_process());
}

#[test]
fn mark_ready() {
    let mut scheduler = Scheduler::new();

    // Incorrect (outdated) pid should be ok.
    scheduler.mark_ready(ProcessId(100));

    let new_actor = actor_fn(simple_actor);
    let rt = ThreadLocal::new(test::runtime());
    let (process, _) = ActorFuture::new(NoSupervisor, new_actor, (), rt).unwrap();
    let pid = scheduler.add_new_process(Priority::NORMAL, process);

    assert!(scheduler.has_process());
    assert!(scheduler.has_ready_process());

    let process = scheduler.next_process().unwrap();
    scheduler.add_back_process(process);
    scheduler.mark_ready(pid);
}

#[test]
fn mark_ready_before_run() {
    let mut scheduler = Scheduler::new();

    // Incorrect (outdated) pid should be ok.
    scheduler.mark_ready(ProcessId(100));

    let pid = add_test_actor(&mut scheduler, Priority::NORMAL);

    assert!(scheduler.has_process());
    assert!(scheduler.has_ready_process());

    let process = scheduler.next_process().unwrap();
    scheduler.mark_ready(pid);
    scheduler.add_back_process(process);
}

#[test]
fn next_process() {
    let mut scheduler = Scheduler::new();

    let pid = add_test_actor(&mut scheduler, Priority::NORMAL);

    if let Some(process) = scheduler.next_process() {
        assert_eq!(process.as_ref().id(), pid);
        assert!(!scheduler.has_process());
        assert!(!scheduler.has_ready_process());
    } else {
        panic!("expected a process");
    }
}

#[test]
fn next_process_order() {
    let mut scheduler = Scheduler::new();

    let pid1 = add_test_actor(&mut scheduler, Priority::LOW);
    let pid2 = add_test_actor(&mut scheduler, Priority::HIGH);
    let pid3 = add_test_actor(&mut scheduler, Priority::NORMAL);

    assert!(scheduler.has_process());
    assert!(scheduler.has_ready_process());

    // Process 2 has a higher priority, should be scheduled first.
    let process2 = scheduler.next_process().unwrap();
    assert_eq!(process2.as_ref().id(), pid2);
    let process3 = scheduler.next_process().unwrap();
    assert_eq!(process3.as_ref().id(), pid3);
    let process1 = scheduler.next_process().unwrap();
    assert_eq!(process1.as_ref().id(), pid1);

    assert!(process1 < process2);
    assert!(process1 < process3);
    assert!(process2 > process1);
    assert!(process2 > process3);
    assert!(process3 > process1);
    assert!(process3 < process2);

    assert_eq!(scheduler.next_process(), None);
}

#[test]
fn add_process() {
    let mut scheduler = Scheduler::new();

    let pid = add_test_actor(&mut scheduler, Priority::NORMAL);

    assert!(scheduler.has_process());
    assert!(scheduler.has_ready_process());

    scheduler.mark_ready(pid);
    assert!(scheduler.has_process());
    assert!(scheduler.has_ready_process());
    let process = scheduler.next_process().unwrap();
    assert_eq!(process.as_ref().id(), pid);
}

#[test]
fn add_process_marked_ready() {
    let mut scheduler = Scheduler::new();

    let pid = add_test_actor(&mut scheduler, Priority::NORMAL);

    let process = scheduler.next_process().unwrap();
    scheduler.add_back_process(process);
    assert!(scheduler.has_process());
    assert!(!scheduler.has_ready_process());

    scheduler.mark_ready(pid);
    assert!(scheduler.has_process());
    assert!(scheduler.has_ready_process());
    let process = scheduler.next_process().unwrap();
    assert_eq!(process.as_ref().id(), pid);
}

#[test]
fn scheduler_run_order() {
    async fn order_actor(
        _: actor::Context<!, ThreadLocal>,
        id: usize,
        order: Rc<RefCell<Vec<usize>>>,
    ) {
        order.borrow_mut().push(id);
    }

    let mut scheduler = Scheduler::new();
    let waker = nop_task_waker();
    let mut ctx = task::Context::from_waker(&waker);

    // The order in which the processes have been run.
    let run_order = Rc::new(RefCell::new(Vec::new()));

    // Add our processes.
    let new_actor = actor_fn(order_actor);
    let priorities = [Priority::LOW, Priority::NORMAL, Priority::HIGH];
    let mut pids = vec![];
    for (id, priority) in priorities.iter().enumerate() {
        let rt = ThreadLocal::new(test::runtime());
        let (process, _) =
            ActorFuture::new(NoSupervisor, new_actor, (id, run_order.clone()), rt).unwrap();
        let pid = scheduler.add_new_process(*priority, process);
        pids.push(pid);
    }

    assert!(scheduler.has_process());
    assert!(scheduler.has_ready_process());

    // Run all processes, should be in order of priority (since there runtimes
    // are equal).
    for _ in 0..3 {
        let mut process = scheduler.next_process().unwrap();
        assert_eq!(process.as_mut().run(&mut ctx), Poll::Ready(()));
    }
    assert!(!scheduler.has_process());
    assert_eq!(*run_order.borrow(), vec![2_usize, 1, 0]);
}

#[test]
fn assert_actor_process_unmoved() {
    let mut scheduler = Scheduler::new();
    let waker = nop_task_waker();
    let mut ctx = task::Context::from_waker(&waker);

    let rt = ThreadLocal::new(test::runtime());
    let (process, _) =
        ActorFuture::new(NoSupervisor, TestAssertUnmovedNewActor::new(), (), rt).unwrap();
    let pid = scheduler.add_new_process(Priority::NORMAL, process);

    // Run the process multiple times, ensure it's not moved in the process.
    let mut process = scheduler.next_process().unwrap();
    assert_eq!(process.as_mut().run(&mut ctx), Poll::Pending);
    scheduler.add_back_process(process);

    scheduler.mark_ready(pid);
    let mut process = scheduler.next_process().unwrap();
    assert_eq!(process.as_mut().run(&mut ctx), Poll::Pending);
    scheduler.add_back_process(process);

    scheduler.mark_ready(pid);
    let mut process = scheduler.next_process().unwrap();
    assert_eq!(process.as_mut().run(&mut ctx), Poll::Pending);
}

#[test]
fn assert_future_process_unmoved() {
    let mut scheduler = Scheduler::new();
    let waker = nop_task_waker();
    let mut ctx = task::Context::from_waker(&waker);

    let process = FutureProcess(AssertUnmoved::new(pending()));
    let _ = scheduler.add_new_process(Priority::NORMAL, process);

    // Run the process multiple times, ensure it's not moved in the process.
    let mut process = scheduler.next_process().unwrap();
    let pid = process.as_ref().id();
    assert_eq!(process.as_mut().run(&mut ctx), Poll::Pending);
    scheduler.add_back_process(process);

    scheduler.mark_ready(pid);
    let mut process = scheduler.next_process().unwrap();
    assert_eq!(process.as_mut().run(&mut ctx), Poll::Pending);
    scheduler.add_back_process(process);

    scheduler.mark_ready(pid);
    let mut process = scheduler.next_process().unwrap();
    assert_eq!(process.as_mut().run(&mut ctx), Poll::Pending);
}

fn add_test_actor(scheduler: &mut Scheduler, priority: Priority) -> ProcessId {
    let new_actor = actor_fn(simple_actor);
    let rt = ThreadLocal::new(test::runtime());
    let (process, _) = ActorFuture::new(NoSupervisor, new_actor, (), rt).unwrap();
    scheduler.add_new_process(priority, process)
}
