use std::sync::mpsc::{Receiver, Sender, channel};

use std::path::PathBuf;

use tapline::{BrowsePage, BrowseQuery, Session, WorkshopItem};

pub const WALLPAPER_ENGINE: tapline_ids::AppId = tapline_ids::AppId(431_960);

#[derive(Debug, Clone)]
pub enum Request {
    Browse(BrowseQuery),
    Count(BrowseQuery),
    WhoAmI,
    SignIn,
    SignOut,
    SubscribeViaClient {
        item: tapline_ids::PublishedFileId,
    },
    Unsubscribe {
        app: tapline_ids::AppId,
        item: tapline_ids::PublishedFileId,
    },
    Install {
        item: Box<WorkshopItem>,
        into: PathBuf,
    },
    EngineAssets {
        into: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub enum Reply {
    Page(Box<BrowsePage>),
    Count(u32),
    Progress { id: u64, done: u64, total: u64 },
    Account { saved: Option<String>, client: bool },
    QrCode(String),
    SignedIn(String),
    SignedOut,
    Subscribed,
    Unsubscribed,
    Installed { id: u64, dir: PathBuf },
    EngineAssets { dir: PathBuf },
    NeedsAccount,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestId(pub u64);

pub struct Workshop {
    outbound: Sender<(RequestId, Request)>,
    inbound: Receiver<(RequestId, Reply)>,
    next: std::cell::Cell<u64>,
    waiting: std::cell::RefCell<Vec<(RequestId, Reply)>>,
}

impl Workshop {
    #[must_use]
    pub fn spawn() -> Self {
        let (outbound, requests) = channel::<(RequestId, Request)>();
        let (replies, inbound) = channel::<(RequestId, Reply)>();

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

    pub fn send(&self, request: Request) -> RequestId {
        let id = RequestId(self.next.get());
        self.next.set(self.next.get().saturating_add(1));
        let _ = self.outbound.send((id, request));
        id
    }

    pub fn take(&self, wanted: RequestId) -> Option<Reply> {
        self.drain();
        let mut waiting = self.waiting.try_borrow_mut().ok()?;
        let at = waiting.iter().position(|(id, _)| *id == wanted)?;
        Some(waiting.remove(at).1)
    }

    pub fn discard(&self, unwanted: RequestId) {
        self.drain();
        if let Ok(mut waiting) = self.waiting.try_borrow_mut() {
            waiting.retain(|(id, _)| *id != unwanted);
        }
    }

    fn drain(&self) {
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

const CLIENT_PATIENCE: std::time::Duration = std::time::Duration::from_secs(20);

const CLIENT_PROBE: std::time::Duration = std::time::Duration::from_secs(4);

fn saved_login() -> Option<String> {
    tapline_auth::TokenStore::default_file()
        .accounts()
        .ok()?
        .into_iter()
        .next()
}

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

fn client() -> Option<tapline_steamworks::Steam> {
    tapline_steamworks::Steam::connect(WALLPAPER_ENGINE).ok()
}

async fn sign_in(
    session: &mut Session,
    replies: &Sender<(RequestId, Reply)>,
    id: RequestId,
) -> Reply {
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
                Err(error) => Reply::Failed(format!(
                    "signed in, but the token could not be saved ({error})"
                )),
            }
        }
        Err(error) => Reply::Failed(error.to_string()),
    }
}

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

    let item_id = item.id.get();
    let outbound = replies.clone();
    let mut last_sent = 0_u64;
    let mut observe = move |event: tapline::Event| {
        if let tapline::Event::Progress {
            bytes_done,
            bytes_total,
        } = event
        {
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
        Err(error) if error.needs_login() => Reply::NeedsAccount,
        Err(error) => Reply::Failed(error.to_string()),
    }
}

async fn engine_assets(
    session: &mut Session,
    into: &std::path::Path,
    replies: &Sender<(RequestId, Reply)>,
    id: RequestId,
) -> Reply {
    let app = tapline_ids::AppId(haru_core::engine::WALLPAPER_ENGINE_APP);
    let options = tapline::InstallOptions {
        install_dir: into.to_owned(),
        ..tapline::InstallOptions::default()
    };

    let outbound = replies.clone();
    let mut last_sent = 0_u64;
    let mut observe = move |event: tapline::Event| {
        if let tapline::Event::Progress {
            bytes_done,
            bytes_total,
        } = event
        {
            let step = bytes_total.max(1) / 200;
            if bytes_done.saturating_sub(last_sent) < step && bytes_done < bytes_total {
                return;
            }
            last_sent = bytes_done;
            let _ = outbound.send((
                id,
                Reply::Progress {
                    id: u64::from(haru_core::engine::WALLPAPER_ENGINE_APP),
                    done: bytes_done,
                    total: bytes_total,
                },
            ));
        }
    };

    match session.install_observed(app, &options, &mut observe).await {
        Ok(_) => Reply::EngineAssets {
            dir: into.join("assets"),
        },
        Err(error) if error.needs_login() => Reply::NeedsAccount,
        Err(error) => Reply::Failed(error.to_string()),
    }
}

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

fn offline(request: &Request, session: &mut Option<Session>) -> Option<Reply> {
    match request {
        Request::WhoAmI => Some(Reply::Account {
            saved: saved_login(),
            client: client_available(),
        }),
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
        Request::EngineAssets { into } => engine_assets(session, &into, replies, id).await,
        Request::WhoAmI | Request::SignOut => {
            Reply::Failed("handled before the session".to_owned())
        }
        Request::SignIn => sign_in(session, replies, id).await,
        Request::SubscribeViaClient { item } => match client() {
            Some(steam) => match steam.subscribe(item, CLIENT_PATIENCE) {
                Ok(()) => Reply::Subscribed,
                Err(error) => Reply::Failed(error.to_string()),
            },
            None => Reply::Failed("no running Steam client to ask".to_owned()),
        },
        Request::Unsubscribe { app, item } => {
            match session.unsubscribe_workshop_item(app, item).await {
                Ok(()) => Reply::Unsubscribed,
                Err(error) if error.needs_login() => Reply::NeedsAccount,
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
            while let Ok((id, _)) = requests.recv() {
                let _ = replies.send((id, Reply::Failed(format!("no runtime: {error}"))));
            }
            return;
        }
    };

    let mut session: Option<Session> = None;

    while let Ok((id, request)) = requests.recv() {
        if let Some(reply) = offline(&request, &mut session) {
            if replies.send((id, reply)).is_err() {
                return;
            }
            continue;
        }

        let mut reply = runtime.block_on(answer(&mut session, request.clone(), replies, id));

        if matches!(&reply, Reply::Failed(why) if was_dropped(why)) {
            session = None;
            reply = runtime.block_on(answer(&mut session, request, replies, id));
        }

        if matches!(&reply, Reply::SignedIn(_) | Reply::NeedsAccount) {
            session = None;
        }

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
