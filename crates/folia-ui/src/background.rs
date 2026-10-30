// Helpers for work that must not run on the UI thread, and for persisting settings.

use std::error::Error;
use std::future::Future;

use folia_store::{Domain, Store};
use iced::Task;
use iced::futures::channel::oneshot;

/// Runs blocking work on its own thread; the future yields `None` only if that thread died.
pub fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> impl Future<Output = Option<T>> {
    let (sender, receiver) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    async move { receiver.await.ok() }
}

/// Saves a settings domain in the background, logging the outcome.
pub fn save<T, M>(store: &Store, value: &T) -> Task<M>
where
    T: Domain + Clone + Send + 'static,
    M: Send + 'static,
{
    let (store, value) = (store.clone(), value.clone());
    Task::future(blocking(move || match store.save(&value) {
        Ok(()) => tracing::debug!(file = T::FILE, "settings saved"),
        Err(err) => tracing::error!(file = T::FILE, error = &err as &dyn Error, "cannot save settings"),
    }))
    .discard()
}

/// `error: cause: cause` on one line, for showing an error chain in the interface.
pub fn error_chain(err: &dyn Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// `mm:ss` like the original player.
pub fn format_time(seconds: f64) -> String {
    let total = if seconds.is_finite() { seconds.max(0.0) as u64 } else { 0 };
    format!("{:02}:{:02}", total / 60, total % 60)
}
