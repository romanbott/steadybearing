use anyhow::bail;
use futures::{SinkExt, StreamExt};
use sb_protocol::types::{HistoricalRow, IpcRequest, IpcResponse, Snapshot};
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

pub type IpcFramed = Framed<UnixStream, LengthDelimitedCodec>;

pub async fn connect(path: &str) -> anyhow::Result<IpcFramed> {
    let stream = UnixStream::connect(path).await?;
    Ok(Framed::new(stream, LengthDelimitedCodec::new()))
}

pub async fn request_snapshot(framed: &mut IpcFramed) -> anyhow::Result<Snapshot> {
    let req_bytes = bincode::serialize(&IpcRequest::SnapshotAll)?;
    framed.send(bytes::Bytes::from(req_bytes)).await?;

    let data = match framed.next().await {
        Some(Ok(d)) => d,
        Some(Err(e)) => bail!("Snapshot recv error: {}", e),
        None => bail!("Connection closed during snapshot"),
    };

    match bincode::deserialize::<IpcResponse>(&data)? {
        IpcResponse::Snapshot(s) => Ok(s),
        other => bail!("Unexpected response: {:?}", other),
    }
}

pub async fn request_stream_live(framed: &mut IpcFramed) -> anyhow::Result<()> {
    let req_bytes = bincode::serialize(&IpcRequest::StreamLive)?;
    framed.send(bytes::Bytes::from(req_bytes)).await?;
    Ok(())
}

pub async fn read_next_tick(
    framed: &mut IpcFramed,
) -> anyhow::Result<sb_protocol::types::MetricSample> {
    let data = match framed.next().await {
        Some(Ok(d)) => d,
        Some(Err(e)) => bail!("Tick recv error: {}", e),
        None => bail!("Connection closed"),
    };

    match bincode::deserialize::<IpcResponse>(&data)? {
        IpcResponse::LiveTick(sample) => Ok(sample),
        other => bail!("Unexpected response: {:?}", other),
    }
}

pub async fn request_historical(
    framed: &mut IpcFramed,
    bucket_secs: u32,
    count: usize,
) -> anyhow::Result<Vec<HistoricalRow>> {
    let req_bytes = bincode::serialize(&IpcRequest::Historical { bucket_secs, count })?;
    framed.send(bytes::Bytes::from(req_bytes)).await?;

    let data = match framed.next().await {
        Some(Ok(d)) => d,
        Some(Err(e)) => bail!("Historical recv error: {}", e),
        None => bail!("Connection closed during historical"),
    };

    match bincode::deserialize::<IpcResponse>(&data)? {
        IpcResponse::Historical(rows) => Ok(rows),
        IpcResponse::Error(message) => bail!("{message}"),
        other => bail!("Unexpected response: {:?}", other),
    }
}
