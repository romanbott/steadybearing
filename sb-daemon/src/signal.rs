use anyhow::Result;
use std::path::PathBuf;

pub struct SignalGuard {
    path: PathBuf,
}

impl SignalGuard {
    pub fn new(pwm_enable_path: PathBuf) -> Result<Self> {
        let path = pwm_enable_path.clone();

        let orig_hook = std::panic::take_hook();
        let panic_path = pwm_enable_path.clone();
        std::panic::set_hook(Box::new(move |info| {
            let _ = Self::restore_auto(&panic_path);
            orig_hook(info);
        }));

        let sig_path = pwm_enable_path.clone();
        ctrlc::set_handler(move || {
            let _ = Self::restore_auto(&sig_path);
            std::process::exit(0);
        })?;

        Ok(SignalGuard { path })
    }

    fn restore_auto(path: &std::path::Path) -> Result<()> {
        std::fs::write(path, b"2\n")?;
        Ok(())
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        let _ = Self::restore_auto(&self.path);
    }
}
