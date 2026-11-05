//! Source maintenance patch for spin_on 0.1.1; see the adjacent README.
// SPDX-License-Identifier: MIT OR Apache-2.0
#![no_std]
#![forbid(unsafe_code)]

use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

/// Poll continuously until a future completes, without parking the thread.
///
/// This preserves spin_on's busy-polling contract. It can consume a full CPU
/// while waiting and is intended for the existing Slint compiler/renderer calls.
pub fn spin_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{cell::Cell, marker::PhantomPinned, pin::Pin};

    #[test]
    fn returns_ready_output() {
        assert_eq!(spin_on(async { 42 }), 42);
    }

    struct PinnedFuture<'a> {
        address: Cell<usize>,
        polls: Cell<u8>,
        drops: &'a Cell<u8>,
        _pin: PhantomPinned,
    }
    impl Future for PinnedFuture<'_> {
        type Output = u8;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u8> {
            let this = self.as_ref().get_ref();
            let address = core::ptr::from_ref(this) as usize;
            let previous = this.address.replace(address);
            assert!(
                previous == 0 || previous == address,
                "future moved after polling"
            );
            let polls = this.polls.get() + 1;
            this.polls.set(polls);
            if polls == 9 {
                Poll::Ready(polls)
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
    impl Drop for PinnedFuture<'_> {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    #[test]
    fn keeps_non_unpin_future_pinned_across_pending_polls_and_drops_once() {
        let drops = Cell::new(0);
        let output = spin_on(PinnedFuture {
            address: Cell::new(0),
            polls: Cell::new(0),
            drops: &drops,
            _pin: PhantomPinned,
        });
        assert_eq!(output, 9);
        assert_eq!(drops.get(), 1);
    }
}
