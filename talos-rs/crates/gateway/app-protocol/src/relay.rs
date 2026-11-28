//! Bounded HTTP transport envelopes for personal-product relay connections.
//! These messages carry existing product HTTP requests, never a second business API.
use agent_protocol::NodeAuth;
use serde::{Deserialize, Serialize};
use talos_error::{ApiError, ErrorCode, TalosError, TalosResult};

pub const RELAY_VERSION: u16 = 1;
pub const RELAY_FILE_TRANSFER_HEADER: &str = "x-talos-file-transfer";
pub const RELAY_CHUNK_BYTES: usize = 64 * 1024;
pub const RELAY_WINDOW_FRAMES: u64 = 8;
pub const RELAY_MAX_STREAMS: usize = 64;
pub const RELAY_MAX_FRAME_BYTES: usize = 128 * 1024;
pub const RELAY_MAX_HEADER_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RelayLane {
    Control,
    Interactive,
    File,
    Media,
}
impl RelayLane {
    pub fn index(self) -> usize {
        match self {
            Self::Control => 0,
            Self::Interactive => 1,
            Self::File => 2,
            Self::Media => 3,
        }
    }
    pub fn for_path(path: &str) -> Self {
        if path.contains("/media") || path.contains("/desktop/stream") {
            Self::Media
        } else if path.contains("/files") || path.contains("/file/") {
            Self::File
        } else {
            Self::Interactive
        }
    }
}
// Headers and payloads may contain credentials or private product data; no Debug.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelayHeader {
    pub name: String,
    pub value: String,
}
#[derive(Clone)]
pub struct RelayCallbackOrigin(pub String);

/// Authentication delegated by the trusted Server, restricted to the attached lease owner.
/// Credentials and authentication bypass flags never cross this envelope.
#[derive(Clone, Serialize, Deserialize)]
pub struct RelayRequestIdentity(#[serde(with = "RelayActorWire")] pub talos_auth::Actor);
#[derive(Serialize, Deserialize)]
#[serde(
    remote = "talos_auth::Actor",
    rename_all = "camelCase",
    deny_unknown_fields
)]
struct RelayActorWire {
    #[serde(skip, default = "remote_product_origin")]
    request_origin: talos_auth::ProductRequestOrigin,
    user_id: String,
    username: Option<String>,
    role: talos_auth::UserRole,
    user_type: talos_auth::UserType,
    #[serde(skip)]
    access_token: Option<String>,
    #[serde(skip)]
    auth_disabled: bool,
}

fn remote_product_origin() -> talos_auth::ProductRequestOrigin {
    talos_auth::ProductRequestOrigin::RemoteProduct
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelayRequestHead {
    pub stream_id: u64,
    pub identity: RelayRequestIdentity,
    pub method: String,
    pub path_and_query: String,
    pub headers: Vec<RelayHeader>,
    pub lane: RelayLane,
    pub callback_origin: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelayResponseHead {
    pub stream_id: u64,
    pub status: u16,
    pub headers: Vec<RelayHeader>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RelayToHost {
    Ready {
        version: u16,
    },
    Request {
        head: RelayRequestHead,
    },
    Data {
        stream_id: u64,
        sequence: u64,
        #[serde(with = "payload")]
        data: Vec<u8>,
    },
    End {
        stream_id: u64,
    },
    Window {
        stream_id: u64,
        acknowledged: u64,
    },
    Cancel {
        stream_id: u64,
    },
    Failure {
        stream_id: u64,
        error: ApiError,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum HostToRelay {
    Hello {
        version: u16,
        host_id: String,
        #[serde(with = "NodeAuthWire")]
        auth: NodeAuth,
    },
    ResponseHead {
        head: RelayResponseHead,
    },
    Data {
        stream_id: u64,
        sequence: u64,
        #[serde(with = "payload")]
        data: Vec<u8>,
    },
    End {
        stream_id: u64,
    },
    Window {
        stream_id: u64,
        acknowledged: u64,
    },
    Cancel {
        stream_id: u64,
    },
    Failure {
        stream_id: u64,
        error: ApiError,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "NodeAuth", rename_all = "camelCase", deny_unknown_fields)]
struct NodeAuthWire {
    node_id: String,
    token: String,
}

impl RelayRequestHead {
    pub fn validate(&self) -> TalosResult<()> {
        if self.identity.0.user_id.is_empty()
            || self.identity.0.user_id.len() > 256
            || self
                .identity
                .0
                .username
                .as_ref()
                .is_some_and(|value| value.len() > 512)
            || self.identity.0.access_token.is_some()
            || self.identity.0.auth_disabled
        {
            return Err(invalid("Product relay identity is invalid."));
        }
        if self
            .callback_origin
            .as_ref()
            .is_some_and(|value| value.len() > 2048)
        {
            return Err(invalid("Product callback origin exceeds its limit."));
        }
        if self.stream_id == 0
            || self.method.is_empty()
            || self.method.len() > 16
            || !self
                .method
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b == b'-')
            || !self.path_and_query.starts_with('/')
            || self.path_and_query.starts_with("//")
            || self.path_and_query.contains('#')
            || self.path_and_query.len() > 8192
        {
            return Err(invalid("Invalid product relay request head."));
        }
        validate_headers(&self.headers)
    }
}
impl RelayResponseHead {
    pub fn validate(&self) -> TalosResult<()> {
        if self.stream_id == 0 || !(100..=599).contains(&self.status) {
            return Err(invalid("Invalid product relay response head."));
        }
        validate_headers(&self.headers)
    }
}
fn validate_headers(headers: &[RelayHeader]) -> TalosResult<()> {
    let total = headers.iter().try_fold(0usize, |size, h| {
        size.checked_add(h.name.len() + h.value.len())
    });
    if headers.len() > 128 || total.is_none_or(|size| size > RELAY_MAX_HEADER_BYTES) {
        return Err(invalid("Product relay headers exceed the transport limit."));
    }
    Ok(())
}
impl RelayToHost {
    pub fn validate(&self) -> TalosResult<()> {
        match self {
            Self::Ready { version } => {
                if *version == RELAY_VERSION {
                    Ok(())
                } else {
                    Err(invalid("Unsupported product relay version."))
                }
            }
            Self::Request { head } => head.validate(),
            Self::Data {
                stream_id,
                sequence,
                data,
            } => validate_data(*stream_id, *sequence, data),
            Self::End { stream_id }
            | Self::Window { stream_id, .. }
            | Self::Cancel { stream_id }
            | Self::Failure { stream_id, .. } => validate_stream(*stream_id),
        }
    }
}
impl HostToRelay {
    pub fn validate(&self) -> TalosResult<()> {
        match self {
            Self::Hello {
                version,
                auth,
                host_id,
            } => {
                if host_id.is_empty()
                    || host_id.len() > 128
                    || *version != RELAY_VERSION
                    || auth.node_id.is_empty()
                    || auth.node_id.len() > 256
                    || auth.token.is_empty()
                    || auth.token.len() > 4096
                {
                    Err(invalid("Invalid product relay authentication envelope."))
                } else {
                    Ok(())
                }
            }
            Self::ResponseHead { head } => head.validate(),
            Self::Data {
                stream_id,
                sequence,
                data,
            } => validate_data(*stream_id, *sequence, data),
            Self::End { stream_id }
            | Self::Window { stream_id, .. }
            | Self::Cancel { stream_id }
            | Self::Failure { stream_id, .. } => validate_stream(*stream_id),
        }
    }
}
fn validate_stream(stream_id: u64) -> TalosResult<()> {
    if stream_id == 0 {
        Err(invalid("Invalid product relay stream identity."))
    } else {
        Ok(())
    }
}
fn validate_data(id: u64, sequence: u64, data: &[u8]) -> TalosResult<()> {
    if id == 0 || sequence == 0 || data.is_empty() || data.len() > RELAY_CHUNK_BYTES {
        return Err(invalid("Invalid product relay data frame."));
    }
    Ok(())
}
fn invalid(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}

/// Sender credits are returned only after the receiver consumes contiguous data.
#[derive(Default)]
pub struct SendWindow {
    sent: u64,
    acknowledged: u64,
}
impl SendWindow {
    pub fn reserve(&mut self) -> Option<u64> {
        if self.sent - self.acknowledged >= RELAY_WINDOW_FRAMES {
            return None;
        }
        self.sent = self.sent.checked_add(1)?;
        Some(self.sent)
    }
    pub fn acknowledge(&mut self, acknowledged: u64) -> TalosResult<()> {
        if acknowledged < self.acknowledged || acknowledged > self.sent {
            return Err(invalid("Invalid product relay acknowledgement."));
        }
        self.acknowledged = acknowledged;
        Ok(())
    }
}
#[derive(Default)]
pub struct ReceiveWindow {
    received: u64,
    consumed: u64,
}
impl ReceiveWindow {
    pub fn receive(&mut self, sequence: u64) -> TalosResult<()> {
        if self.received.checked_add(1) != Some(sequence)
            || self.received - self.consumed >= RELAY_WINDOW_FRAMES
        {
            return Err(invalid("Product relay data exceeded its credit window."));
        }
        self.received = sequence;
        Ok(())
    }
    pub fn consume(&mut self, sequence: u64) -> TalosResult<u64> {
        if self.consumed.checked_add(1) != Some(sequence) || sequence > self.received {
            return Err(invalid("Invalid product relay consumption order."));
        }
        self.consumed = sequence;
        Ok(sequence)
    }
}
mod payload {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(data: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(data))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let raw = String::deserialize(d)?;
        if raw.len() > super::RELAY_CHUNK_BYTES.div_ceil(3) * 4 {
            return Err(serde::de::Error::custom(
                "Product relay chunk is too large.",
            ));
        }
        STANDARD.decode(raw).map_err(serde::de::Error::custom)
    }
}
#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
