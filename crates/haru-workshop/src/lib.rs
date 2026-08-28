//! Talking to the Workshop without blocking a frame.
//!
//! tapline's API is async and owns a live connection to a Steam CM. egui draws
//! sixty times a second on one thread and cannot await anything. So the
//! connection lives on a worker thread with its own runtime, and the two sides
//! exchange values over channels: the UI asks, keeps drawing, and picks up the
//! answer whenever it arrives.
//!
//! One session, reused. Opening a CM connection costs about a second, which is
//! most of what a search costs, and a picker searches constantly.
//!
//! ```no_run
//! # fn example() {
//! use haru_workshop::{Request, Workshop};
//! use tapline::{AppId, BrowseQuery};
//!
//! let workshop = Workshop::spawn();
//! let id = workshop.send(Request::Browse(BrowseQuery {
//!     app: AppId(431_960),
//!     ..BrowseQuery::default()
//! }));
//!
//! // …frames happen…
//! while let Some((answered, reply)) = workshop.poll() {
//!     if answered == id {
//!         println!("{reply:?}");
//!     }
//! }
//! # }
//! ```

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use tapline::{BrowsePage, BrowseQuery, Session};

/// Wallpaper Engine, the Workshop haru exists for.
pub const WALLPAPER_ENGINE: tapline_ids::AppId = tapline_ids::AppId(431_960);

/// What the UI wants done.
#[derive(Debug, Clone)]
pub enum Request {
    /// Fetch a page of results.
    Browse(BrowseQuery),
    /// Count what a query matches, without fetching any of it.
    Count(BrowseQuery),
}

/// What came back.
#[derive(Debug, Clone)]
pub enum Reply {
    /// A page of results.
    Page(Box<BrowsePage>),
    /// How many a query matched.
    Count(u32),
    /// Steam, or the connection, said no.
    Failed(String),
}

/// Identifies one request, so a late answer to an abandoned search can be
/// dropped rather than drawn.
///
/// A picker changes its mind constantly — every keystroke that ends in Enter,
/// every filter click — and answers arrive in whatever order Steam manages.
/// Without this, a slow first search overwrites the fast second one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestId(u64);

/// A live connection to the Workshop, driven from a UI thread.
pub struct Workshop {
    outbound: Sender<(RequestId, Request)>,
    inbound: Receiver<(RequestId, Reply)>,
    next: std::cell::Cell<u64>,
}

impl Workshop {
    /// Starts the worker and its runtime.
    ///
    /// Never fails: a machine with no network still gets a window, and the
    /// first search says what went wrong. Failing here would mean a picker
    /// that refuses to open because Steam was unreachable at launch.
    #[must_use]
    pub fn spawn() -> Self {
        let (outbound, requests) = channel::<(RequestId, Request)>();
        let (replies, inbound) = channel::<(RequestId, Reply)>();

        // Detached deliberately: when the UI drops its end, the worker's next
        // recv fails and the thread returns. There is nothing to join.
        std::thread::Builder::new()
            .name("haru-workshop".to_owned())
            .spawn(move || worker(&requests, &replies))
            .ok();

        Self {
            outbound,
            inbound,
            next: std::cell::Cell::new(1),
        }
    }

    /// Queues a request and returns the id its answer will carry.
    pub fn send(&self, request: Request) -> RequestId {
        let id = RequestId(self.next.get());
        self.next.set(self.next.get().saturating_add(1));
        // A dead worker is not an error worth propagating from here: the reply
        // simply never arrives, and the caller is already drawing a spinner.
        let _ = self.outbound.send((id, request));
        id
    }

    /// Takes one answer, if any has arrived. Never blocks.
    pub fn poll(&self) -> Option<(RequestId, Reply)> {
        match self.inbound.try_recv() {
            Ok(answer) => Some(answer),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }
}

/// The worker: one runtime, one session, requests in order.
fn worker(requests: &Receiver<(RequestId, Request)>, replies: &Sender<(RequestId, Reply)>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            // Every request fails the same way, with the reason, rather than
            // the UI waiting forever on a thread that died at startup.
            while let Ok((id, _)) = requests.recv() {
                let _ = replies.send((id, Reply::Failed(format!("no runtime: {error}"))));
            }
            return;
        }
    };

    let mut session: Option<Session> = None;

    while let Ok((id, request)) = requests.recv() {
        let reply = runtime.block_on(async {
            // Opened on first use and kept: the connection costs about a
            // second, which is most of what a search costs.
            if session.is_none() {
                match Session::automatic(None).await {
                    Ok(open) => session = Some(open),
                    Err(error) => return Reply::Failed(error.to_string()),
                }
            }
            let Some(session) = session.as_mut() else {
                return Reply::Failed("no session".to_owned());
            };

            match request {
                Request::Browse(query) => match session.browse_workshop(&query).await {
                    Ok(page) => Reply::Page(Box::new(page)),
                    Err(error) => Reply::Failed(error.to_string()),
                },
                Request::Count(query) => match session.count_workshop(&query).await {
                    Ok(total) => Reply::Count(total),
                    Err(error) => Reply::Failed(error.to_string()),
                },
            }
        });

        // A dropped UI ends the worker; there is nobody left to answer.
        if replies.send((id, reply)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_unique_and_ordered() {
        // The UI compares them to decide whether an answer is still wanted, so
        // a repeat would draw a search the user has already moved on from.
        let workshop = Workshop::spawn();
        let first = workshop.send(Request::Count(BrowseQuery::default()));
        let second = workshop.send(Request::Count(BrowseQuery::default()));
        assert!(second > first);
    }

    #[test]
    fn polling_an_idle_workshop_does_not_block() {
        let workshop = Workshop::spawn();
        assert!(workshop.poll().is_none());
    }
}
