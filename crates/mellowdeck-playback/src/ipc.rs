use mellowdeck_core::{AppError, DeviceId, ErrorKind, Result, SpotifyUri};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const BRIDGE_PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Envelope<T> {
    pub version: u16,
    pub id: u64,
    pub payload: T,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    Ready {
        device_id: DeviceId,
    },
    Unavailable {
        reason: Option<String>,
    },
    StateChanged {
        playing: bool,
        position_ms: u64,
        duration_ms: u64,
        track_uri: Option<SpotifyUri>,
    },
    AuthenticationError {
        message: String,
    },
    PlaybackError {
        message: String,
    },
    AccountError {
        message: String,
    },
}

#[derive(Deserialize)]
struct UntrustedEnvelope {
    version: u16,
    id: u64,
    payload: Value,
}

/// Parses an untrusted, versioned event from the embedded playback surface.
///
/// # Errors
///
/// Returns an error for oversized input, invalid JSON, an unsupported protocol version, an
/// unknown event type, or invalid domain identifiers.
pub fn parse_event(json: &str) -> Result<Envelope<BridgeEvent>> {
    if json.len() > 64 * 1024 {
        return Err(AppError::new(ErrorKind::InvalidInput, "player event is too large"));
    }
    let envelope: UntrustedEnvelope = serde_json::from_str(json)
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "malformed player event"))?;
    if envelope.version != BRIDGE_PROTOCOL_VERSION {
        return Err(AppError::new(ErrorKind::InvalidInput, "unsupported player protocol version"));
    }
    let payload = serde_json::from_value(envelope.payload)
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "unknown or malformed player event"))?;
    Ok(Envelope { version: envelope.version, id: envelope.id, payload })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_versioned_event() {
        let event =
            parse_event(r#"{"version":1,"id":7,"payload":{"type":"ready","device_id":"abc123"}}"#)
                .unwrap();
        assert_eq!(
            event.payload,
            BridgeEvent::Ready { device_id: DeviceId::parse("abc123").unwrap() }
        );
    }

    #[test]
    fn rejects_unknown_version_and_event() {
        assert!(
            parse_event(r#"{"version":2,"id":7,"payload":{"type":"ready","device_id":"abc"}}"#)
                .is_err()
        );
        assert!(
            parse_event(r#"{"version":1,"id":7,"payload":{"type":"execute_script"}}"#).is_err()
        );
    }

    #[test]
    fn rejects_oversized_messages() {
        assert!(parse_event(&"x".repeat(64 * 1024 + 1)).is_err());
    }
}
