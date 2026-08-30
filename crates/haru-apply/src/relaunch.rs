use std::path::{Path, PathBuf};

use crate::launch::{self, DESKTOP, Plan};
use crate::{Backend, Screen};

pub struct Relaunch {
    socket: PathBuf,
    showing: std::sync::Mutex<Option<PathBuf>>,
}

impl Relaunch {
    #[must_use]
    pub fn new(socket: PathBuf) -> Self {
        Self {
            socket,
            showing: std::sync::Mutex::new(None),
        }
    }

    fn binary(&self) -> Result<PathBuf, String> {
        crate::install::installed().ok_or_else(|| "no renderer installed yet".to_owned())
    }
}

impl Backend for Relaunch {
    fn name(&self) -> &'static str {
        "kirie"
    }

    fn available(&self) -> bool {
        self.binary().is_ok()
    }

    fn screens(&self) -> Result<Vec<Screen>, String> {
        let current = self
            .showing
            .lock()
            .ok()
            .and_then(|showing| showing.clone())
            .filter(|_| launch::running());
        Ok(vec![Screen {
            name: DESKTOP.to_owned(),
            current,
        }])
    }

    fn apply(&self, _screen: &str, dir: &Path) -> Result<(), String> {
        let plan = [Plan::showing(DESKTOP, dir)];
        launch::restart(&self.binary()?, &self.socket, &plan)?;
        if let Ok(mut showing) = self.showing.lock() {
            *showing = Some(dir.to_path_buf());
        }
        Ok(())
    }

    fn tune(&self, _commands: &[String]) -> Result<(), String> {
        let showing = self
            .showing
            .lock()
            .ok()
            .and_then(|showing| showing.clone())
            .filter(|dir| dir.is_dir());
        let Some(dir) = showing else {
            return Ok(());
        };
        if !launch::running() {
            return Ok(());
        }
        launch::restart(
            &self.binary()?,
            &self.socket,
            &[Plan::showing(DESKTOP, &dir)],
        )
    }

    fn set_property(&self, _screen: &str, _key: &str, _value: &str) -> Result<(), String> {
        Err("changing a property while it runs needs the control socket, which this platform does not have yet".to_owned())
    }

    fn stage(&self, _key: &str, _value: &str) -> Result<(), String> {
        Err(
            "staged properties need the control socket, which this platform does not have yet"
                .to_owned(),
        )
    }
}
