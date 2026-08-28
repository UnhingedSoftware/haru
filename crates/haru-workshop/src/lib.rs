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

use std::sync::mpsc::{Receiver, Sender, channel};

use std::path::PathBuf;

use tapline::{BrowsePage, BrowseQuery, Session, WorkshopItem};

/// Wallpaper Engine, the Workshop haru exists for.
pub const WALLPAPER_ENGINE: tapline_ids::AppId = tapline_ids::AppId(431_960);

/// What the UI wants done.
#[derive(Debug, Clone)]
pub enum Request {
    /// Fetch a page of results.
    Browse(BrowseQuery),
    /// Count what a query matches, without fetching any of it.
    Count(BrowseQuery),
    /// Tell Steam this account no longer wants an item.
    Unsubscribe {
        /// Which app's Workshop.
        app: tapline_ids::AppId,
        /// Which item.
        item: tapline_ids::PublishedFileId,
    },
    /// Download one item into a Steam library.
    Install {
        /// What to fetch, exactly as a search returned it — no second lookup.
        item: Box<WorkshopItem>,
        /// The Steam library root it lands under.
        into: PathBuf,
    },
}

/// What came back.
#[derive(Debug, Clone)]
pub enum Reply {
    /// A page of results.
    Page(Box<BrowsePage>),
    /// How many a query matched.
    Count(u32),
    /// How far a download has got, in bytes.
    Progress {
        /// Which item.
        id: u64,
        /// Bytes written so far.
        done: u64,
        /// Bytes expected, when the plan knows.
        total: u64,
    },
    /// Steam has been told the account no longer wants an item.
    Unsubscribed,
    /// An item is on disk, at this directory.
    Installed {
        /// Which item.
        id: u64,
        /// Where it landed.
        dir: PathBuf,
    },
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
    ///
    /// A disconnected worker reads the same as an empty queue on purpose: the
    /// UI's response to both is to keep drawing, and a dead worker has already
    /// shown itself through whatever request never came back.
    pub fn poll(&self) -> Option<(RequestId, Reply)> {
        self.inbound.try_recv().ok()
    }
}

/// Downloads one item, reporting progress as it goes.
///
/// The layout is steamcmd's — `<library>/steamapps/workshop/content/431960/<id>`
/// — because that is where the Steam client, Wallpaper Engine and kirie all
/// already look. Installing anywhere else would mean an item nothing but haru
/// can see.
async fn install(
    session: &mut Session,
    item: &WorkshopItem,
    into: &std::path::Path,
    replies: &Sender<(RequestId, Reply)>,
    id: RequestId,
) -> Reply {
    let options = tapline::InstallOptions {
        install_dir: into.to_owned(),
        ..tapline::InstallOptions::default()
    };
    let dir = tapline::item_dir(into, item.app, item.id);

    // Progress goes out as it happens rather than being collected: a 90 MB
    // wallpaper is thirty seconds of nothing otherwise. tapline emits one of
    // these per chunk written, which is far more often than a window needs, so
    // only a change worth drawing is forwarded.
    let item_id = item.id.get();
    let outbound = replies.clone();
    let mut last_sent = 0_u64;
    let mut observe = move |event: tapline::Event| {
        if let tapline::Event::Progress {
            bytes_done,
            bytes_total,
        } = event
        {
            // Every half-percent, or the last one.
            let step = bytes_total.max(1) / 200;
            if bytes_done.saturating_sub(last_sent) < step && bytes_done < bytes_total {
                return;
            }
            last_sent = bytes_done;
            let _ = outbound.send((
                id,
                Reply::Progress {
                    id: item_id,
                    done: bytes_done,
                    total: bytes_total,
                },
            ));
        }
    };

    match session
        .download_workshop_item_observed(item, &options, &mut observe)
        .await
    {
        Ok(_) => Reply::Installed { id: item_id, dir },
        // The one failure worth saying plainly: Wallpaper Engine's depot is
        // not anonymously accessible, and the fix is one command.
        Err(error) if error.needs_login() => Reply::Failed(
            "this needs a Steam account that owns Wallpaper Engine — run `tapline login --qr` once"
                .to_owned(),
        ),
        Err(error) => Reply::Failed(error.to_string()),
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
                Request::Install { item, into } => {
                    install(session, &item, &into, replies, id).await
                }
                Request::Unsubscribe { app, item } => {
                    match session.unsubscribe_workshop_item(app, item).await {
                        Ok(()) => Reply::Unsubscribed,
                        // Subscriptions belong to an account, so an anonymous
                        // session has none to change — worth saying plainly,
                        // because the files can still be removed.
                        Err(error) if error.needs_login() => Reply::Failed(
                            "unsubscribing needs a signed-in account — run `tapline login --qr`"
                                .to_owned(),
                        ),
                        Err(error) => Reply::Failed(error.to_string()),
                    }
                }
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
