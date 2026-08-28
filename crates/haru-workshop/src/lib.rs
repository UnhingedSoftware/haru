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
//! if let Some(reply) = workshop.take(id) {
//!     println!("{reply:?}");
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
    /// Report who is signed in, and whether a Steam client is reachable.
    WhoAmI,
    /// Sign in by QR code, and save the token for next time.
    SignIn,
    /// Forget the saved login and drop the connection using it.
    SignOut,
    /// Subscribe through a running Steam client, which needs no login here.
    SubscribeViaClient {
        /// Which item.
        item: tapline_ids::PublishedFileId,
    },
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
    /// What routes to Steam exist on this machine.
    Account {
        /// Who is signed in on this machine, if anyone.
        saved: Option<String>,
        /// Whether a running Steam client can be reached.
        client: bool,
    },
    /// A QR code to scan, as a URL. Sent again whenever Steam rotates it.
    ///
    /// Rotation is the part that is easy to miss: a code expires mid-login and
    /// Steam hands back a new one, so a window that drew only the first would
    /// show an unscannable square and wait forever.
    QrCode(String),
    /// Signed in, as this account.
    SignedIn(String),
    /// The saved login is gone.
    SignedOut,
    /// A running Steam client was told to subscribe; it downloads from here.
    Subscribed,
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
pub struct RequestId(pub u64);

/// A live connection to the Workshop, driven from a UI thread.
pub struct Workshop {
    outbound: Sender<(RequestId, Request)>,
    inbound: Receiver<(RequestId, Reply)>,
    next: std::cell::Cell<u64>,
    /// Answers that have arrived and not yet been claimed.
    ///
    /// More than one view shares this connection, so an answer is claimed by
    /// the request that asked for it rather than by whoever polls first. An
    /// earlier version had views hand back what was not theirs, which
    /// deadlocked the moment nobody wanted something: the unclaimed reply was
    /// served again on the next poll, ahead of everything behind it, for ever.
    waiting: std::cell::RefCell<Vec<(RequestId, Reply)>>,
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
            waiting: std::cell::RefCell::new(Vec::new()),
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

    /// Takes the answer to one request, if it has arrived. Never blocks.
    ///
    /// Answers to anything else are left where they are for whoever asked.
    pub fn take(&self, wanted: RequestId) -> Option<Reply> {
        self.drain();
        let mut waiting = self.waiting.try_borrow_mut().ok()?;
        let at = waiting.iter().position(|(id, _)| *id == wanted)?;
        Some(waiting.remove(at).1)
    }

    /// Forgets any answers to a request nobody is waiting for any more.
    ///
    /// A search replaced by a newer one, say. Without this its reply would sit
    /// in the buffer until the process ended.
    pub fn discard(&self, unwanted: RequestId) {
        self.drain();
        if let Ok(mut waiting) = self.waiting.try_borrow_mut() {
            waiting.retain(|(id, _)| *id != unwanted);
        }
    }

    /// Moves whatever has arrived into the buffer.
    ///
    /// Bounded: an answer nobody ever claims — a view closed before its reply
    /// landed — would otherwise accumulate for the life of the process.
    fn drain(&self) {
        /// How many unclaimed answers to keep before dropping the oldest.
        const KEEP: usize = 64;

        let Ok(mut waiting) = self.waiting.try_borrow_mut() else {
            return;
        };
        while let Ok(answer) = self.inbound.try_recv() {
            waiting.push(answer);
        }
        if waiting.len() > KEEP {
            let excess = waiting.len() - KEEP;
            waiting.drain(..excess);
        }
    }
}

/// How long to wait on a client that is thinking about it.
const CLIENT_PATIENCE: std::time::Duration = std::time::Duration::from_secs(20);

/// How long to wait for a Steam client to say whether it is there.
///
/// Not optional. Measured on this machine: `SteamAPI_Init` loads
/// `steamclient.so`, logs `[API loaded no]`, and sometimes never returns —
/// 45 seconds and counting, against a Steam that was plainly running. A window
/// cannot wait on that, so the answer after this long is "no client".
const CLIENT_PROBE: std::time::Duration = std::time::Duration::from_secs(4);

/// Who has a login saved on this machine, if anyone.
///
/// tapline's own answer rather than the file's existence, which is what this
/// used to check: `accounts()` reads the store without touching Steam and
/// names what it finds, so the window can say who it is signed in as instead
/// of only that it is.
///
/// More than one can be saved. haru signs in one at a time, so the first is
/// the one it is using.
fn saved_login() -> Option<String> {
    tapline_auth::TokenStore::default_file()
        .accounts()
        .ok()?
        .into_iter()
        .next()
}

/// Whether a running Steam client can be reached, without hanging on it.
///
/// The connect happens on its own thread and is given a deadline. A thread
/// left behind by an init that never returns is one thread, once, for the life
/// of the process — the alternative is a window that never answers because a
/// game client is thinking.
fn client_available() -> bool {
    let (answer, heard) = channel();
    std::thread::Builder::new()
        .name("haru-steam-probe".to_owned())
        .spawn(move || {
            let reachable = tapline_steamworks::Steam::connect(WALLPAPER_ENGINE).is_ok();
            let _ = answer.send(reachable);
        })
        .ok();
    heard.recv_timeout(CLIENT_PROBE).unwrap_or(false)
}

/// The running Steam client, for work that needs one.
///
/// Allowed to block, unlike [`client_available`]: it runs when something has
/// already been asked of the client, where waiting is the expected cost.
fn client() -> Option<tapline_steamworks::Steam> {
    tapline_steamworks::Steam::connect(WALLPAPER_ENGINE).ok()
}

/// Signs in by QR code, reporting each code as it is issued.
///
/// tapline owns the loop and the refresh; this forwards the codes to whatever
/// is drawing them and saves the token at the end, so the next run signs in by
/// itself.
async fn sign_in(
    session: &mut Session,
    replies: &Sender<(RequestId, Reply)>,
    id: RequestId,
) -> Reply {
    /// Long enough to find a phone, short enough not to hang for ever.
    const PATIENCE: std::time::Duration = std::time::Duration::from_secs(180);

    let outbound = replies.clone();
    let mut on_code = move |url: &str| {
        let _ = outbound.send((id, Reply::QrCode(url.to_owned())));
    };

    match session.qr_login(PATIENCE, &mut on_code).await {
        Ok(token) => {
            let account = token.account.clone();
            match tapline_auth::TokenStore::default_file().save(&token) {
                Ok(()) => Reply::SignedIn(account),
                // Signed in but not saved: this session works, the next one
                // asks again, and saying so beats a silent re-prompt later.
                Err(error) => Reply::Failed(format!(
                    "signed in, but the token could not be saved ({error})"
                )),
            }
        }
        Err(error) => Reply::Failed(error.to_string()),
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
    // these per chunk written, far more often than a window needs, so only a
    // change worth drawing is forwarded.
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
            "this needs a Steam account that owns Wallpaper Engine — sign in, or open Steam"
                .to_owned(),
        ),
        Err(error) => Reply::Failed(error.to_string()),
    }
}

/// The worker: one runtime, one session, requests in order.
/// Opens the connection if it is not open, and serves one request over it.
///
/// Kept open between requests: it costs about a second, which is most of what
/// a search costs.
async fn answer(
    session: &mut Option<Session>,
    request: Request,
    replies: &Sender<(RequestId, Reply)>,
    id: RequestId,
) -> Reply {
    if session.is_none() {
        match Session::automatic(None).await {
            Ok(open) => *session = Some(open),
            Err(error) => return Reply::Failed(error.to_string()),
        }
    }
    let Some(open) = session.as_mut() else {
        return Reply::Failed("no session".to_owned());
    };
    serve(request, open, replies, id).await
}

/// Whether a failure is the connection having gone away rather than the
/// request being wrong.
///
/// Matched on the message because that is what a session hands back. The cost
/// of being wrong either way is one extra reconnection, or one failure that
/// could have been retried — never a wrong answer.
fn was_dropped(why: &str) -> bool {
    let why = why.to_ascii_lowercase();
    [
        "connection closed",
        "connection reset",
        "broken pipe",
        "not connected",
    ]
    .iter()
    .any(|sign| why.contains(sign))
}

/// What can be answered without a Steam connection.
///
/// Returns `None` when the request needs one. Both of these used to be handled
/// inside the session block, which meant a slow handshake left the account
/// state unresolved for as long as it took — for questions a file and a
/// process could answer on their own.
fn offline(request: &Request, session: &mut Option<Session>) -> Option<Reply> {
    match request {
        Request::WhoAmI => Some(Reply::Account {
            saved: saved_login(),
            client: client_available(),
        }),
        // Forgetting a login is a file, and dropping the connection that used
        // it is the other half — without that, this process would keep working
        // as the account it was just told to forget.
        Request::SignOut => Some(
            match tapline_auth::TokenStore::default_file().forget_all() {
                Ok(()) => {
                    *session = None;
                    Reply::SignedOut
                }
                Err(error) => Reply::Failed(error.to_string()),
            },
        ),
        _ => None,
    }
}

/// Everything that needs the connection, once there is one.
async fn serve(
    request: Request,
    session: &mut Session,
    replies: &Sender<(RequestId, Reply)>,
    id: RequestId,
) -> Reply {
    match request {
        Request::Browse(query) => match session.browse_workshop(&query).await {
            Ok(page) => Reply::Page(Box::new(page)),
            Err(error) => Reply::Failed(error.to_string()),
        },
        Request::Count(query) => match session.count_workshop(&query).await {
            Ok(total) => Reply::Count(total),
            Err(error) => Reply::Failed(error.to_string()),
        },
        Request::Install { item, into } => install(session, &item, &into, replies, id).await,
        // Both are answered before the session; see above.
        Request::WhoAmI | Request::SignOut => {
            Reply::Failed("handled before the session".to_owned())
        }
        Request::SignIn => sign_in(session, replies, id).await,
        Request::SubscribeViaClient { item } => match client() {
            // Steam downloads it into its own library, which is where
            // this looks anyway — no depot key, no login here.
            Some(steam) => match steam.subscribe(item, CLIENT_PATIENCE) {
                Ok(()) => Reply::Subscribed,
                Err(error) => Reply::Failed(error.to_string()),
            },
            None => Reply::Failed("no running Steam client to ask".to_owned()),
        },
        Request::Unsubscribe { app, item } => {
            match session.unsubscribe_workshop_item(app, item).await {
                Ok(()) => Reply::Unsubscribed,
                // Subscriptions belong to an account, so an anonymous
                // session has none to change — worth saying plainly,
                // because the files can still be removed.
                Err(error) if error.needs_login() => Reply::Failed(
                    "unsubscribing needs a signed-in account — run `tapline login --qr`".to_owned(),
                ),
                Err(error) => Reply::Failed(error.to_string()),
            }
        }
    }
}

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
        // Answered without a connection, and before one is opened: a token
        // file and a process check need no Steam, and going through the
        // session made the window wait on a CM handshake to find out whether a
        // file exists.
        if let Some(reply) = offline(&request, &mut session) {
            if replies.send((id, reply)).is_err() {
                return;
            }
            continue;
        }

        let mut reply = runtime.block_on(answer(&mut session, request.clone(), replies, id));

        // A connection that has been sitting idle gets closed by the far end,
        // and the first request after that fails on a socket nobody told us
        // about. Once, and only for that: opening a new one costs a second,
        // and a request that failed on its own merits must not be sent twice.
        if matches!(&reply, Reply::Failed(why) if was_dropped(why)) {
            session = None;
            reply = runtime.block_on(answer(&mut session, request, replies, id));
        }

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
    fn an_idle_workshop_answers_nothing_and_does_not_block() {
        let workshop = Workshop::spawn();
        assert!(workshop.take(RequestId(1)).is_none());
    }

    #[test]
    fn an_unclaimed_answer_does_not_block_the_ones_behind_it() {
        // The failure this replaced: a view handed back a reply that was not
        // its own, the next poll served the same one first, and every answer
        // behind it waited for ever.
        let workshop = Workshop::spawn();
        if let Ok(mut waiting) = workshop.waiting.try_borrow_mut() {
            waiting.push((RequestId(1), Reply::Unsubscribed));
            waiting.push((RequestId(2), Reply::Subscribed));
        }
        assert!(
            matches!(workshop.take(RequestId(2)), Some(Reply::Subscribed)),
            "an answer must be reachable past an unclaimed one"
        );
        assert!(matches!(
            workshop.take(RequestId(1)),
            Some(Reply::Unsubscribed)
        ));
    }

    #[test]
    fn unclaimed_answers_do_not_pile_up_for_ever() {
        let workshop = Workshop::spawn();
        if let Ok(mut waiting) = workshop.waiting.try_borrow_mut() {
            for id in 0..200 {
                waiting.push((RequestId(id), Reply::Unsubscribed));
            }
        }
        workshop.drain();
        assert!(
            workshop.waiting.borrow().len() <= 64,
            "the buffer must be bounded"
        );
    }
}
