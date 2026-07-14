use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Notify;

pub struct SignalGuard {
    path: PathBuf,
}

impl SignalGuard {
    pub fn new(pwm_enable_path: PathBuf) -> anyhow::Result<(Self, Arc<Notify>)> {
        let notify = Arc::new(Notify::new());

        let panic_path = pwm_enable_path.clone();
        let panic_notify = notify.clone();
        let orig_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore_auto(&panic_path);
            panic_notify.notify_one();
            orig_hook(info);
        }));

        let sig_path = pwm_enable_path.clone();
        let sig_notify = notify.clone();
        ctrlc::set_handler(move || {
            let _ = restore_auto(&sig_path);
            sig_notify.notify_one();
        })?;

        Ok((SignalGuard { path: pwm_enable_path }, notify))
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        let _ = restore_auto(&self.path);
    }
}

fn restore_auto(path: &std::path::Path) -> anyhow::Result<()> {
    std::fs::write(path, b"2\n")?;
    Ok(())
}
