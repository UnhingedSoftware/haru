use std::sync::mpsc::{Receiver, TryRecvError, channel};

use haru_apply::install::{self, Build};
use haru_apply::update;

enum Word {
    Note(String),
    Landed(Landed),
    Done(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landed {
    Haru,
    Renderer,
}

#[derive(Default)]
pub struct Updates {
    working: Option<Receiver<Word>>,
    note: String,
    fresh: Option<String>,
    due: Option<std::time::Instant>,
    haru_ready: Option<String>,
    renderer_ready: Option<String>,
}

pub const BETWEEN_CHECKS: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

#[must_use]
pub fn due_now(now: std::time::Instant, due: Option<std::time::Instant>) -> bool {
    due.is_none_or(|at| now >= at)
}

impl Updates {
    pub fn take_news(&mut self) -> Option<String> {
        self.fresh.take()
    }
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

    #[must_use]
    pub fn ready(&self) -> (Option<&str>, Option<&str>) {
        (self.haru_ready.as_deref(), self.renderer_ready.as_deref())
    }

    pub fn forget_renderer(&mut self) {
        self.renderer_ready = None;
    }

    pub fn tick(&mut self, wanted: bool, betas: bool) {
        self.collect();
        if self.busy() || !due_now(std::time::Instant::now(), self.due) {
            return;
        }
        self.due = Some(std::time::Instant::now() + BETWEEN_CHECKS);

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
            .spawn(move || work(&say, missing, wanted, betas));
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
                Ok(Word::Landed(what)) => match what {
                    Landed::Haru => self.haru_ready = Some(self.note.clone()),
                    Landed::Renderer => self.renderer_ready = Some(self.note.clone()),
                },
                Ok(Word::Done(note)) => {
                    if !note.is_empty() && note != "everything is up to date" {
                        self.fresh = Some(note.clone());
                    }
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

fn work(say: &std::sync::mpsc::Sender<Word>, missing: bool, updating: bool, betas: bool) {
    if missing {
        let wanted = if betas {
            install::latest_including_betas(install::Web::suggested())
        } else {
            install::latest(install::Web::suggested())
        };
        let note = match wanted {
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
        match update::renderer_update(&binary, betas) {
            Ok(Some(build)) => said.push(take(say, &build, false)),
            Ok(None) => {}
            Err(why) => said.push(format!("could not check the renderer: {why}")),
        }
    }
    if let Ok(Some(build)) = update::haru_update(betas) {
        said.push(take(say, &build, true));
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
        Ok(_) => {
            let note = if myself {
                format!("haru {} is ready — restart it", build.tag)
            } else {
                format!("the renderer is now {}", build.tag)
            };
            let _ = say.send(Word::Note(note.clone()));
            let _ = say.send(Word::Landed(if myself {
                Landed::Haru
            } else {
                Landed::Renderer
            }));
            note
        }
        Err(why) => format!("could not fetch {what}: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn the_first_check_happens_right_away() {
        assert!(due_now(Instant::now(), None));
    }

    #[test]
    fn a_scheduled_check_waits_its_turn() {
        let now = Instant::now();
        assert!(!due_now(now, Some(now + Duration::from_secs(60))));
        assert!(due_now(now, Some(now - Duration::from_secs(1))));
    }

    #[test]
    fn checks_are_spaced_by_hours_not_seconds() {
        assert!(BETWEEN_CHECKS >= Duration::from_secs(60 * 60));
    }

    #[test]
    fn nothing_is_waiting_on_a_fresh_start() {
        let updates = Updates::default();
        assert_eq!(updates.ready(), (None, None));
    }
}
