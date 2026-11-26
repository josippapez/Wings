//! A wait with a limit, for the agent tools in both frames.

use std::{future::Future, task::Poll};
use wings_plugin::host;

/// What `future` gives, or none once `ms` have passed. The timer only runs while this waits.
pub async fn within<T>(ms: u32, future: impl Future<Output = T>) -> Option<T> {
    let mut future = Box::pin(future);
    let mut timer = Box::pin(host::sleep(ms));
    std::future::poll_fn(move |cx| {
        if let Poll::Ready(value) = future.as_mut().poll(cx) {
            return Poll::Ready(Some(value));
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}
