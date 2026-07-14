use std::os::unix::fs::PermissionsExt;

use futures::{SinkExt, StreamExt};
use tokio::net::UnixListener;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use sb_protocol::types::{HistoricalRow, IpcRequest, IpcResponse, MetricSample};

use crate::config::Config;
use crate::store::TelemetryStore;

pub fn spawn(
    store: TelemetryStore,
    socket_path: String,
    tx: broadcast::Sender<MetricSample>,
    config: Config,
) -> JoinHandle<anyhow::Result<()>> {
    tokio::spawn(async move {
        let _ = tokio::fs::remove_file(&socket_path).await;
        if let Some(parent) = std::path::Path::new(&socket_path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o666))?;

        loop {
            let (stream, _addr) = listener.accept().await?;
            let store = store.clone();
            let tx = tx.clone();
            let config = config.clone();

            tokio::spawn(async move {
                let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

                while let Some(result) = framed.next().await {
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
                        IpcRequest::Historical { bucket_secs, count } => {
                            let db_path = if bucket_secs >= 3600 {
                                config.db_archive_path.clone()
                            } else {
                                config.db_path.clone()
                            };

                            let rows = match tokio::task::spawn_blocking(move || {
                                query_historical(&db_path, count)
                            })
                            .await
                            {
                                Ok(Ok(rows)) => rows,
                                Ok(Err(e)) => {
                                    tracing::error!(?e, "historical query failed");
                                    continue;
                                }
                                Err(e) => {
                                    tracing::error!(?e, "spawn_blocking join failed");
                                    continue;
                                }
                            };

                            let response = IpcResponse::Historical(rows);
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
    })
}

fn query_historical(path: &str, count: usize) -> anyhow::Result<Vec<HistoricalRow>> {
    let conn = rusqlite::Connection::open(path)?;
    let mut stmt = conn.prepare(
        "SELECT bucket_ts, temp_min, temp_max, temp_mean,
                usage_min, usage_max, usage_mean, rpm_mean, pwm_mean
         FROM rollups ORDER BY bucket_ts DESC LIMIT ?1",
    )?;
    let mut rows: Vec<HistoricalRow> = stmt
        .query_map(rusqlite::params![count as i64], |row| {
            Ok(HistoricalRow {
                timestamp: row.get(0)?,
                temp_min: row.get::<_, f64>(1)? as f32,
                temp_max: row.get::<_, f64>(2)? as f32,
                temp_mean: row.get::<_, f64>(3)? as f32,
                usage_min: row.get::<_, f64>(4)? as f32,
                usage_max: row.get::<_, f64>(5)? as f32,
                usage_mean: row.get::<_, f64>(6)? as f32,
                rpm_mean: row.get::<_, f64>(7)? as f32,
                pwm_mean: row.get::<_, f64>(8)? as f32,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    rows.reverse();
    Ok(rows)
}
