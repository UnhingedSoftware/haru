use std::sync::mpsc::{Receiver, TryRecvError, channel};

use haru_apply::install::{self, Build};
use haru_apply::update;

enum Word {
    Note(String),
    Done(String),
}

#[derive(Default)]
pub struct Updates {
    asked: bool,
    working: Option<Receiver<Word>>,
    note: String,
}

impl Updates {
    #[must_use]
    pub fn note(&self) -> &str {
        &self.note
    }

    #[must_use]
    pub fn busy(&self) -> bool {
        self.working.is_some()
    }

    /// Fetch the renderer if it is missing, then keep both up to date. Runs once
    /// per launch; everything slow happens on its own thread.
    pub fn tick(&mut self, wanted: bool) {
        self.collect();
        if self.asked || self.busy() {
            return;
        }
        self.asked = true;

        let missing = install::installed().is_none();
        if !missing && !wanted {
            return;
        }
        if !install::supported() {
            self.note = "no build of the renderer for this platform yet".to_owned();
            return;
        }

        let (say, heard) = channel();
        self.working = Some(heard);
        self.note = if missing {
            "fetching the renderer…".to_owned()
        } else {
            "checking for updates…".to_owned()
        };

        let started = std::thread::Builder::new()
            .name("haru-updates".to_owned())
            .spawn(move || work(&say, missing, wanted));
        if started.is_err() {
            self.working = None;
            self.note = "could not start the update thread".to_owned();
        }
    }

    fn collect(&mut self) {
        let Some(heard) = &self.working else {
            return;
        };
        loop {
            match heard.try_recv() {
                Ok(Word::Note(note)) => self.note = note,
                Ok(Word::Done(note)) => {
                    self.note = note;
                    self.working = None;
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.working = None;
                    return;
                }
            }
        }
    }
}

fn work(say: &std::sync::mpsc::Sender<Word>, missing: bool, updating: bool) {
    if missing {
        let note = match install::latest(install::Web::suggested()) {
            Ok(build) => take(say, &build, false),
            Err(why) => format!("could not reach the renderer's releases: {why}"),
        };
        let _ = say.send(Word::Done(note));
        return;
    }
    if !updating {
        let _ = say.send(Word::Done(String::new()));
        return;
    }

    let mut said: Vec<String> = Vec::new();
    if let Some(binary) = install::installed() {
        match update::renderer_update(&binary) {
            Ok(Some(build)) => said.push(take(say, &build, false)),
            Ok(None) => {}
            Err(why) => said.push(format!("could not check the renderer: {why}")),
        }
    }
    match update::haru_update() {
        Ok(Some(build)) => said.push(take(say, &build, true)),
        Ok(None) => {}
        Err(why) => said.push(format!("could not check haru: {why}")),
    }

    let note = if said.is_empty() {
        "everything is up to date".to_owned()
    } else {
        said.join(" · ")
    };
    let _ = say.send(Word::Done(note));
}

fn take(say: &std::sync::mpsc::Sender<Word>, build: &Build, myself: bool) -> String {
    let what = if myself { "haru" } else { "the renderer" };
    let mut progress = |done: u64, size: u64| {
        let share = done
            .saturating_mul(100)
            .checked_div(size)
            .map(|percent| format!(" {percent}%"))
            .unwrap_or_default();
        let _ = say.send(Word::Note(format!("fetching {what} {}{share}", build.tag)));
    };

    let taken = if myself {
        update::take_haru(build, &mut progress)
    } else {
        update::take_renderer(build, &mut progress)
    };
    match taken {
        Ok(_) if myself => format!("haru {} is ready — restart it", build.tag),
        Ok(_) => format!("the renderer is now {}", build.tag),
        Err(why) => format!("could not fetch {what}: {why}"),
    }
}
