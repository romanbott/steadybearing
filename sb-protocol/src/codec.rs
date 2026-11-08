// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Roman Contreras

//! Wire encoding for IPC messages.
//!
//! Messages are encoded with `bincode-next`'s serde support. The
//! configuration is defined once, here, so the daemon and the TUI cannot
//! drift apart: [`bincode_next::config::legacy`] (little endian, fixed-width
//! integers), which is byte-compatible with bincode 1.x. Framing already
//! bounds a message through the length-delimited codec's default maximum
//! frame length.
//!
//! Changing the configuration or the shape of a message changes the wire
//! format; bump [`crate::types::PROTOCOL_VERSION`] when that happens.

use std::fmt;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Error returned when encoding or decoding an IPC message.
#[derive(Debug)]
pub enum CodecError {
    /// The value could not be serialized.
    Encode(bincode_next::error::EncodeError),
    /// The bytes could not be deserialized.
    Decode(bincode_next::error::DecodeError),
    /// The frame held more bytes than the value consumed.
    TrailingBytes { message_len: usize, consumed: usize },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(err) => write!(f, "encoding IPC message failed: {err}"),
            Self::Decode(err) => write!(f, "decoding IPC message failed: {err}"),
            Self::TrailingBytes {
                message_len,
                consumed,
            } => write!(
                f,
                "decoding IPC message failed: {consumed} of {message_len} bytes consumed"
            ),
        }
    }
}

impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(err) => Some(err),
            Self::Decode(err) => Some(err),
            Self::TrailingBytes { .. } => None,
        }
    }
}

/// Encode a message into a frame payload.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    bincode_next::serde::encode_to_vec(value, bincode_next::config::legacy())
        .map_err(CodecError::Encode)
}

/// Decode a message from a frame payload.
///
/// The whole payload must be consumed: a valid prefix followed by extra bytes
/// is an error, matching the strictness of the previous `bincode` 1 codec.
pub fn decode<T: DeserializeOwned>(payload: &[u8]) -> Result<T, CodecError> {
    let (value, consumed) =
        bincode_next::serde::decode_from_slice(payload, bincode_next::config::legacy())
            .map_err(CodecError::Decode)?;

    if consumed != payload.len() {
        return Err(CodecError::TrailingBytes {
            message_len: payload.len(),
            consumed,
        });
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, CodecError};
    use crate::types::{
        HistoricalResolution, HistoricalRow, IpcRequest, IpcResponse, MetricSample,
        PROTOCOL_VERSION,
    };

    fn round_trip<T>(value: &T) -> T
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let bytes = encode(value).expect("encode");
        decode(&bytes).expect("decode")
    }

    #[test]
    fn metric_sample_round_trips() {
        let sample = MetricSample {
            timestamp: 1_767_225_600_000,
            cpu_temp: 48.5,
            cpu_usage: 12.25,
            fan_rpm: 2100,
            pwm_value: 110,
        };

        let decoded = round_trip(&sample);

        assert_eq!(decoded.timestamp, sample.timestamp);
        assert_eq!(decoded.cpu_temp, sample.cpu_temp);
        assert_eq!(decoded.cpu_usage, sample.cpu_usage);
        assert_eq!(decoded.fan_rpm, sample.fan_rpm);
        assert_eq!(decoded.pwm_value, sample.pwm_value);
    }

    #[test]
    fn historical_row_round_trips() {
        let row = HistoricalRow {
            timestamp: 1_767_225_600,
            temp_min: 41.0,
            temp_max: 57.5,
            temp_mean: 49.25,
            usage_min: 2.0,
            usage_max: 88.5,
            usage_mean: 33.75,
            rpm_mean: 1850.5,
            pwm_mean: 96.25,
        };

        let decoded = round_trip(&row);

        assert_eq!(decoded.timestamp, row.timestamp);
        assert_eq!(decoded.temp_min, row.temp_min);
        assert_eq!(decoded.temp_max, row.temp_max);
        assert_eq!(decoded.temp_mean, row.temp_mean);
        assert_eq!(decoded.usage_min, row.usage_min);
        assert_eq!(decoded.usage_max, row.usage_max);
        assert_eq!(decoded.usage_mean, row.usage_mean);
        assert_eq!(decoded.rpm_mean, row.rpm_mean);
        assert_eq!(decoded.pwm_mean, row.pwm_mean);
    }

    #[test]
    fn every_request_round_trips() {
        let requests = [
            IpcRequest::Hello {
                protocol: PROTOCOL_VERSION,
            },
            IpcRequest::SnapshotAll,
            IpcRequest::StreamLive,
            IpcRequest::Historical {
                resolution: HistoricalResolution::TenSecond,
                count: 0,
            },
            IpcRequest::Historical {
                resolution: HistoricalResolution::Minute,
                count: 720,
            },
            IpcRequest::Historical {
                resolution: HistoricalResolution::TenMinute,
                count: 144,
            },
            IpcRequest::Historical {
                resolution: HistoricalResolution::Hourly,
                count: usize::MAX,
            },
        ];

        for request in &requests {
            assert_eq!(round_trip(request), *request);
        }
    }

    #[test]
    fn every_response_round_trips() {
        let sample = MetricSample {
            timestamp: 1_767_225_600_000,
            cpu_temp: 48.5,
            cpu_usage: 12.25,
            fan_rpm: 2100,
            pwm_value: 110,
        };
        let row = HistoricalRow {
            timestamp: 1_767_225_600,
            temp_min: 41.0,
            temp_max: 57.5,
            temp_mean: 49.25,
            usage_min: 2.0,
            usage_max: 88.5,
            usage_mean: 33.75,
            rpm_mean: 1850.5,
            pwm_mean: 96.25,
        };
        let responses = [
            IpcResponse::Hello {
                protocol: PROTOCOL_VERSION,
            },
            IpcResponse::Snapshot(vec![sample]),
            IpcResponse::Snapshot(Vec::new()),
            IpcResponse::LiveTick(sample),
            IpcResponse::Historical(vec![row]),
            IpcResponse::Historical(Vec::new()),
            IpcResponse::Error("protocol mismatch".to_string()),
            IpcResponse::Error(String::new()),
        ];

        for response in &responses {
            assert_eq!(round_trip(response), *response);
        }
    }

    #[test]
    fn truncated_payload_is_rejected() {
        let bytes = encode(&IpcRequest::SnapshotAll).expect("encode");
        let truncated = &bytes[..bytes.len() - 1];

        assert!(matches!(
            decode::<IpcRequest>(truncated),
            Err(CodecError::Decode(_))
        ));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut bytes = encode(&IpcRequest::StreamLive).expect("encode");
        let message_len = bytes.len();
        bytes.push(0);

        match decode::<IpcRequest>(&bytes) {
            Err(CodecError::TrailingBytes {
                message_len: len,
                consumed,
            }) => {
                assert_eq!(len, message_len + 1);
                assert_eq!(consumed, message_len);
            }
            other => panic!("expected trailing bytes error, got {other:?}"),
        }
    }
}
