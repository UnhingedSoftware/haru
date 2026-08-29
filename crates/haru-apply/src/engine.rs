use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::launch::Plan;
use crate::{Backend, Screen, install, launch};

const POLL: Duration = Duration::from_millis(900);

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub available: bool,
    pub screens: Vec<Screen>,
    pub connectors: Vec<String>,
    pub pid: Option<u32>,
    pub binary: Option<PathBuf>,
    pub working: bool,
}

enum Job {
    Apply {
        screen: String,
        dir: PathBuf,
        staged: Vec<(String, String)>,
    },
    Property {
        screen: String,
        key: String,
        value: String,
    },
    Start(Vec<Plan>),
    Restart(Vec<Plan>),
    Stop,
}

pub struct Engine {
    jobs: Sender<Job>,
    shared: Arc<Mutex<Snapshot>>,
    notes: Receiver<Result<String, String>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::spawn(None)
    }
}

impl Engine {
    #[must_use]
    pub fn spawn(socket: Option<PathBuf>) -> Self {
        let (jobs, queue) = channel();
        let (notes_out, notes) = channel();
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let held = Arc::clone(&shared);

        let spawned = std::thread::Builder::new()
            .name("haru-engine".to_owned())
            .spawn(move || {
                let path = socket.clone().unwrap_or_else(crate::kirie::default_socket);
                let backend = crate::for_this_platform(socket);
                worker(backend.as_ref(), &path, &queue, &held, &notes_out);
            });
        if spawned.is_err() {
            if let Ok(mut snapshot) = shared.lock() {
                snapshot.binary = install::installed();
            }
        }

        Self {
            jobs,
            shared,
            notes,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        self.shared
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default()
    }

    pub fn take_note(&self) -> Option<Result<String, String>> {
        self.notes.try_recv().ok()
    }

    pub fn apply(&self, screen: &str, dir: &Path, staged: Vec<(String, String)>) {
        self.working();
        let _ = self.jobs.send(Job::Apply {
            screen: screen.to_owned(),
            dir: dir.to_owned(),
            staged,
        });
    }

    pub fn property(&self, screen: &str, key: &str, value: &str) {
        let _ = self.jobs.send(Job::Property {
            screen: screen.to_owned(),
            key: key.to_owned(),
            value: value.to_owned(),
        });
    }

    pub fn start(&self, plan: Vec<Plan>) {
        self.working();
        let _ = self.jobs.send(Job::Start(plan));
    }

    pub fn restart(&self, plan: Vec<Plan>) {
        self.working();
        let _ = self.jobs.send(Job::Restart(plan));
    }

    pub fn stop(&self) {
        self.working();
        let _ = self.jobs.send(Job::Stop);
    }

    fn working(&self) {
        if let Ok(mut snapshot) = self.shared.lock() {
            snapshot.working = true;
        }
    }
}

fn worker(
    engine: &dyn Backend,
    socket: &Path,
    queue: &Receiver<Job>,
    shared: &Arc<Mutex<Snapshot>>,
    notes: &Sender<Result<String, String>>,
) {
    poll(engine, shared);
    loop {
        match queue.recv_timeout(POLL) {
            Ok(job) => {
                let note = run(engine, socket, job, shared);
                if notes.send(note).is_err() {
                    return;
                }
                poll(engine, shared);
                if let Ok(mut snapshot) = shared.lock() {
                    snapshot.working = false;
                }
            }
            Err(RecvTimeoutError::Timeout) => poll(engine, shared),
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn run(
    engine: &dyn Backend,
    socket: &Path,
    job: Job,
    shared: &Arc<Mutex<Snapshot>>,
) -> Result<String, String> {
    match job {
        Job::Apply {
            screen,
            dir,
            staged,
        } => {
            for (key, value) in staged {
                let _ = engine.stage(&key, &value);
            }
            engine
                .apply(&screen, &dir)
                .map(|()| format!("applied to {screen}"))
        }
        Job::Property { screen, key, value } => engine
            .set_property(&screen, &key, &value)
            .map(|()| format!("{key} = {value}")),
        Job::Start(plan) => start(socket, shared, &plan, false),
        Job::Restart(plan) => start(socket, shared, &plan, true),
        Job::Stop => launch::stop().map(|()| "the renderer is stopped".to_owned()),
    }
}

fn start(
    socket: &Path,
    shared: &Arc<Mutex<Snapshot>>,
    plan: &[Plan],
    replacing: bool,
) -> Result<String, String> {
    let binary = shared
        .lock()
        .ok()
        .and_then(|held| held.binary.clone())
        .or_else(install::installed)
        .ok_or("no renderer installed yet")?;

    let outcome = if replacing || launch::running() {
        launch::restart(&binary, socket, plan)
    } else {
        launch::start(&binary, socket, plan)
    };
    outcome.map(|()| "the renderer is up".to_owned())
}

fn poll(engine: &dyn Backend, shared: &Arc<Mutex<Snapshot>>) {
    let available = engine.available();
    let screens = if available {
        engine.screens().unwrap_or_default()
    } else {
        Vec::new()
    };
    let pid = launch::pid();
    let binary = install::installed();
    let connectors = launch::connectors();

    if let Ok(mut snapshot) = shared.lock() {
        snapshot.available = available;
        snapshot.screens = screens;
        snapshot.connectors = connectors;
        snapshot.pid = pid;
        snapshot.binary = binary;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_readable_before_the_first_poll_answers() {
        let engine = Engine::spawn(Some(PathBuf::from("/nonexistent/haru-engine.sock")));
        let snapshot = engine.snapshot();
        assert!(!snapshot.available);
        assert!(snapshot.screens.is_empty());
    }

    #[test]
    fn asking_never_blocks_on_a_socket_that_is_not_there() {
        let engine = Engine::spawn(Some(PathBuf::from("/nonexistent/haru-engine.sock")));
        let started = std::time::Instant::now();
        engine.apply("DP-1", Path::new("/tmp"), Vec::new());
        engine.property("DP-1", "speed", "2");
        engine.stop();
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "the caller waited {:?}",
            started.elapsed()
        );
    }
}
