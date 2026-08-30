use std::path::{Path, PathBuf};

use crate::launch::{self, DESKTOP, Plan};
use crate::{Backend, Screen};

pub struct Relaunch {
    socket: PathBuf,
    live: crate::Kirie,
    showing: std::sync::Mutex<Option<PathBuf>>,
}

impl Relaunch {
    #[must_use]
    pub fn new(socket: PathBuf) -> Self {
        Self {
            live: crate::Kirie::new(Some(socket.clone())),
            socket,
            showing: std::sync::Mutex::new(None),
        }
    }

    fn binary(&self) -> Result<PathBuf, String> {
        crate::install::installed().ok_or_else(|| "no renderer installed yet".to_owned())
    }

    fn speaking(&self) -> Option<&crate::Kirie> {
        self.live.available().then_some(&self.live)
    }

    fn put_up_again(&self) -> Result<(), String> {
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
}

impl Backend for Relaunch {
    fn name(&self) -> &'static str {
        "kirie"
    }

    fn available(&self) -> bool {
        self.binary().is_ok()
    }

    fn screens(&self) -> Result<Vec<Screen>, String> {
        if let Some(live) = self.speaking()
            && let Ok(screens) = live.screens()
            && !screens.is_empty()
        {
            return Ok(screens);
        }
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

    fn apply(&self, screen: &str, dir: &Path) -> Result<(), String> {
        match self.speaking() {
            Some(live) => live.apply(screen, dir)?,
            None => {
                let plan = [Plan::showing(DESKTOP, dir)];
                launch::restart(&self.binary()?, &self.socket, &plan)?;
            }
        }
        if let Ok(mut showing) = self.showing.lock() {
            *showing = Some(dir.to_path_buf());
        }
        Ok(())
    }

    fn tune(&self, commands: &[String]) -> Result<(), String> {
        match self.speaking() {
            Some(live) => live.tune(commands),
            None => self.put_up_again(),
        }
    }

    fn set_property(&self, screen: &str, key: &str, value: &str) -> Result<(), String> {
        match self.speaking() {
            Some(live) => live.set_property(screen, key, value),
            None => Err("the renderer is not running, so there is nothing to change".to_owned()),
        }
    }

    fn stage(&self, key: &str, value: &str) -> Result<(), String> {
        match self.speaking() {
            Some(live) => live.stage(key, value),
            None => Ok(()),
        }
    }
}
