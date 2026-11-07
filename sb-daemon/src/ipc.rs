use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use futures::{SinkExt, StreamExt};
use tokio::net::UnixListener;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use sb_protocol::types::{IpcRequest, IpcResponse, MetricSample};

use crate::config::Config;
use crate::store::TelemetryStore;

const MAX_CONNECTIONS: usize = 64;
const MAX_HISTORICAL_ROWS: usize = 100_000;
const FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Held for the lifetime of the daemon. The OS releases the lock when the
/// process exits, so the file itself is intentionally left in place.
pub struct InstanceLock {
    _file: std::fs::File,
}

impl InstanceLock {
    pub fn acquire(socket_path: &str) -> anyhow::Result<Self> {
        let path = format!("{socket_path}.lock");
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;

        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                anyhow::bail!("another sb-daemon is already running (lock: {path})")
            }
            Err(std::fs::TryLockError::Error(err)) => Err(err.into()),
        }
    }
}

struct SocketCleanup {
    path: String,
}

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn spawn(
    store: TelemetryStore,
    socket_path: String,
    tx: broadcast::Sender<MetricSample>,
    config: Config,
) -> anyhow::Result<JoinHandle<anyhow::Result<()>>> {
    let lock = InstanceLock::acquire(&socket_path)?;

    Ok(tokio::spawn(async move {
        let _lock = lock;
        let _ = tokio::fs::remove_file(&socket_path).await;
        if let Some(parent) = std::path::Path::new(&socket_path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        let _cleanup = SocketCleanup {
            path: socket_path.clone(),
        };
        if let Some(gid) = config.socket_gid {
            std::os::unix::fs::chown(&socket_path, None, Some(gid))
                .with_context(|| format!("changing socket group to {gid}"))?;
        }
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o660))?;

        let connections = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));

        loop {
            let (stream, _addr) = match listener.accept().await {
                Ok(pair) => pair,
                Err(err) => {
                    tracing::error!(?err, "ipc accept failed");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            let permit = match connections.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    tracing::warn!("ipc connection limit reached");
                    continue;
                }
            };

            let store = store.clone();
            let tx = tx.clone();
            let config = config.clone();

            tokio::spawn(async move {
                let _permit = permit;
                let mut framed = Framed::new(stream, LengthDelimitedCodec::new());
                let mut first_request = true;

                loop {
                    let next = if std::mem::take(&mut first_request) {
                        match tokio::time::timeout(
                            FIRST_REQUEST_TIMEOUT,
                            framed.next(),
                        )
                        .await
                        {
                            Ok(next) => next,
                            Err(_) => {
                                tracing::debug!("ipc client timed out before its first request");
                                return;
                            }
                        }
                    } else {
                        framed.next().await
                    };

                    let Some(result) = next else { return };
                    let data = match result {
                        Ok(d) => d,
                        Err(_) => return,
                    };

                    let request: IpcRequest = match bincode::deserialize(&data) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };

                    match request {
                        IpcRequest::SnapshotAll => {
                            let snapshot = sb_protocol::types::Snapshot {
                                tier1: store.tier1.snapshot(),
                                tier2: store.tier2.snapshot(),
                                tier3: store.tier3.snapshot(),
                                tier4: store.tier4.snapshot(),
                            };
                            let response = IpcResponse::Snapshot(snapshot);
                            let response_bytes =
                                bincode::serialize(&response).unwrap_or_default();
                            let _ = framed
                                .send(bytes::Bytes::from(response_bytes))
                                .await;
                        }
                        IpcRequest::StreamLive => {
                            let mut rx = tx.subscribe();

                            loop {
                                match rx.recv().await {
                                    Ok(sample) => {
                                        let response = IpcResponse::LiveTick(sample);
                                        let response_bytes =
                                            bincode::serialize(&response).unwrap_or_default();
                                        if framed
                                            .send(bytes::Bytes::from(response_bytes))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                    Err(broadcast::error::RecvError::Lagged(n)) => {
                                        tracing::warn!(lagged = n, "broadcast lagged");
                                        continue;
                                    }
                                    Err(broadcast::error::RecvError::Closed) => break,
                                }
                            }
                        }
                        IpcRequest::Historical { resolution, count } => {
                            let db_path = config.db_path.clone();
                            let count = count.min(MAX_HISTORICAL_ROWS);

                            let result = tokio::task::spawn_blocking(move || {
                                crate::db::query_historical(&db_path, resolution, count)
                            })
                            .await;

                            let response = match result {
                                Ok(Ok(rows)) => IpcResponse::Historical(rows),
                                Ok(Err(e)) => {
                                    tracing::error!(?e, "historical query failed");
                                    IpcResponse::Error(format!("historical query failed: {e}"))
                                }
                                Err(e) => {
                                    tracing::error!(?e, "spawn_blocking join failed");
                                    IpcResponse::Error(format!(
                                        "historical query task failed: {e}"
                                    ))
                                }
                            };

                            let response_bytes =
                                bincode::serialize(&response).unwrap_or_default();
                            let _ = framed
                                .send(bytes::Bytes::from(response_bytes))
                                .await;
                        }
                    }
                }
            });
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::InstanceLock;

    #[test]
    fn only_one_instance_can_hold_the_lock() {
        let path = std::env::temp_dir().join(format!(
            "sb-lock-test-{}.sock",
            std::process::id()
        ));
        let path_str = path.to_string_lossy().to_string();
        let lock_path = format!("{path_str}.lock");
        let _ = std::fs::remove_file(&lock_path);

        let first = InstanceLock::acquire(&path_str).expect("first lock");
        assert!(InstanceLock::acquire(&path_str).is_err());

        drop(first);
        assert!(InstanceLock::acquire(&path_str).is_ok());

        let _ = std::fs::remove_file(&lock_path);
    }
}
