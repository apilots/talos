//! Bounded client-side RFB 3.8 framing. Authority is fixed when a stream is created.
//! Clipboard, desktop resizing and unknown extensions never reach the private server.
use std::collections::BTreeSet;
use talos_error::{ErrorCode, TalosError, TalosResult};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Handshake {
    Version,
    Security,
    Shared,
    Ready,
}

pub(crate) struct RfbInput {
    handshake: Handshake,
    pending: Vec<u8>,
    control: bool,
    keys: BTreeSet<u32>,
    extended_keys: BTreeSet<(u32, u32)>,
    pointer: Option<(u16, u16)>,
}
pub(crate) struct RfbMessage {
    pub data: Vec<u8>,
    pub initializes: bool,
}

impl RfbInput {
    pub fn new(control: bool) -> Self {
        Self {
            handshake: Handshake::Version,
            pending: Vec::new(),
            control,
            keys: BTreeSet::new(),
            extended_keys: BTreeSet::new(),
            pointer: None,
        }
    }

    /// Preserve complete message boundaries. The caller fences each returned
    /// message before writing it; bytes cannot be relabelled with a later epoch.
    pub fn receive(&mut self, chunk: &[u8]) -> TalosResult<Vec<RfbMessage>> {
        if chunk.len() > 64 * 1024 || self.pending.len() + chunk.len() > 128 * 1024 {
            return Err(rejected());
        }
        self.pending.extend_from_slice(chunk);
        let mut messages = Vec::new();
        loop {
            let length = match self.handshake {
                Handshake::Version => 12,
                Handshake::Security | Handshake::Shared => 1,
                Handshake::Ready => {
                    let Some(kind) = self.pending.first().copied() else {
                        break;
                    };
                    match kind {
                        0 => 20, // SetPixelFormat
                        2 => {
                            if self.pending.len() < 4 {
                                break;
                            }
                            let count =
                                u16::from_be_bytes([self.pending[2], self.pending[3]]) as usize;
                            if count > 256 {
                                return Err(rejected());
                            }
                            4 + count * 4
                        }
                        3 => 10, // FramebufferUpdateRequest
                        4 if self.control => 8,
                        5 if self.control => 6,
                        150 => 10, // EnableContinuousUpdates
                        248 => {
                            if self.pending.len() < 9 {
                                break;
                            }
                            let length = self.pending[8] as usize;
                            if length > 64 {
                                return Err(rejected());
                            }
                            9 + length // Fence response
                        }
                        255 if self.control => {
                            if self.pending.len() < 2 {
                                break;
                            }
                            if self.pending[1] != 0 {
                                return Err(rejected());
                            }
                            12 // QEMU ExtendedKeyEvent
                        }
                        _ => return Err(rejected()),
                    }
                }
            };
            if self.pending.len() < length {
                break;
            }
            let mut message: Vec<u8> = self.pending.drain(..length).collect();
            let initializes = self.handshake == Handshake::Shared;
            match self.handshake {
                Handshake::Version => {
                    if message != b"RFB 003.008\n" {
                        return Err(rejected());
                    }
                    self.handshake = Handshake::Security;
                }
                Handshake::Security => {
                    if message != [1] {
                        return Err(rejected());
                    }
                    self.handshake = Handshake::Shared;
                }
                Handshake::Shared => {
                    if message != [1] {
                        return Err(rejected());
                    }
                    self.handshake = Handshake::Ready;
                }
                Handshake::Ready => {
                    if message[0] == 2 {
                        // Negotiate only supported input extensions. In particular,
                        // no clipboard capability response should ever be required.
                        let encodings = message[4..]
                            .chunks_exact(4)
                            .filter(|bytes| {
                                let id =
                                    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                                !matches!(id, 0xc0a1e5ce | 0xfffffec4 | 0xfffffecb)
                            })
                            .flat_map(|bytes| bytes.iter().copied())
                            .collect::<Vec<_>>();
                        message = vec![2, 0];
                        message.extend_from_slice(&((encodings.len() / 4) as u16).to_be_bytes());
                        message.extend(encodings);
                    }
                }
            }
            messages.push(RfbMessage {
                data: message,
                initializes,
            });
        }
        Ok(messages)
    }

    /// Track only the primitive admitted for actual dispatch, never future
    /// messages parsed from the same chunk. An uncertain send retains its hold.
    pub fn prepare_send(&mut self, message: &[u8]) -> TalosResult<()> {
        match message[0] {
            0 if !matches!(message[4], 8 | 16 | 32) => return Err(rejected()),
            4 => {
                let key = u32::from_be_bytes(message[4..8].try_into().map_err(|_| rejected())?);
                if message[1] == 1 {
                    self.keys.insert(key);
                } else if message[1] != 0 {
                    return Err(rejected());
                }
            }
            5 => {
                let x = u16::from_be_bytes([message[2], message[3]]);
                let y = u16::from_be_bytes([message[4], message[5]]);
                if x >= 1600 || y >= 1000 {
                    return Err(rejected());
                }
                if message[1] != 0 {
                    self.pointer = Some((x, y));
                }
            }
            255 => {
                let key = u32::from_be_bytes(message[4..8].try_into().map_err(|_| rejected())?);
                let code = u32::from_be_bytes(message[8..12].try_into().map_err(|_| rejected())?);
                match u16::from_be_bytes([message[2], message[3]]) {
                    0 => {}
                    1 => {
                        self.extended_keys.insert((key, code));
                    }
                    _ => return Err(rejected()),
                }
            }
            _ => {}
        }
        if self.keys.len() + self.extended_keys.len() > 256 {
            return Err(rejected());
        }
        Ok(())
    }

    /// A release stops being held only after the private write completed.
    pub fn sent(&mut self, message: &[u8]) {
        match message[0] {
            4 if message[1] == 0 => {
                self.keys
                    .remove(&u32::from_be_bytes(message[4..8].try_into().unwrap()));
            }
            5 if message[1] == 0 => self.pointer = None,
            255 if message[2..4] == [0, 0] => {
                let key = u32::from_be_bytes(message[4..8].try_into().unwrap());
                let code = u32::from_be_bytes(message[8..12].try_into().unwrap());
                self.extended_keys.remove(&(key, code));
            }
            _ => {}
        }
    }

    pub fn releases(&self) -> Vec<Vec<u8>> {
        let mut messages = Vec::new();
        for key in &self.keys {
            let mut message = vec![4, 0, 0, 0];
            message.extend_from_slice(&key.to_be_bytes());
            messages.push(message);
        }
        for (key, code) in &self.extended_keys {
            let mut message = vec![255, 0, 0, 0];
            message.extend_from_slice(&key.to_be_bytes());
            message.extend_from_slice(&code.to_be_bytes());
            messages.push(message);
        }
        if let Some((x, y)) = self.pointer {
            let mut message = vec![5, 0];
            message.extend_from_slice(&x.to_be_bytes());
            message.extend_from_slice(&y.to_be_bytes());
            messages.push(message);
        }
        messages
    }
}

fn rejected() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "Desktop input is not permitted on this stream.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ready(control: bool) -> RfbInput {
        let mut input = RfbInput::new(control);
        for byte in b"RFB 003.008\n\x01\x01" {
            input.receive(&[*byte]).unwrap();
        }
        input
    }
    #[test]
    fn readonly_rejects_fragmented_input_clipboard_resize_and_unknown_extensions() {
        for message in [vec![4], vec![5], vec![6], vec![251], vec![255], vec![99]] {
            assert!(ready(false).receive(&message).is_err());
        }
        let mut input = ready(false);
        assert!(input.receive(&[2, 0, 1]).unwrap().is_empty());
        assert!(input.receive(&[1]).is_err()); // 257 encodings exceeds the limit.
    }
    #[test]
    fn control_releases_every_held_key_and_button_after_fragmented_messages() {
        let mut input = ready(true);
        let key = [4, 1, 0, 0, 0, 0, 0xff, 0xe1];
        for part in key.chunks(3) {
            for message in input.receive(part).unwrap() {
                input.prepare_send(&message.data).unwrap();
                input.sent(&message.data);
            }
        }
        input.prepare_send(&[5, 1, 0, 4, 0, 5]).unwrap();
        assert_eq!(
            input.releases(),
            vec![vec![4, 0, 0, 0, 0, 0, 0xff, 0xe1], vec![5, 0, 0, 4, 0, 5]]
        );
        input.sent(&[4, 0, 0, 0, 0, 0, 0xff, 0xe1]);
        input.sent(&[5, 0, 0, 4, 0, 5]);
        assert!(input.releases().is_empty());
    }

    #[test]
    fn batched_future_releases_cannot_erase_a_partially_dispatched_hold() {
        let mut input = ready(true);
        let batch = [
            4, 1, 0, 0, 0, 0, 0xff, 0xe1, 5, 1, 0, 4, 0, 5, 4, 0, 0, 0, 0, 0, 0xff, 0xe1, 5, 0, 0,
            4, 0, 5,
        ];
        let messages = input.receive(&batch).unwrap();
        assert!(input.releases().is_empty());
        for message in &messages[..2] {
            input.prepare_send(&message.data).unwrap();
            input.sent(&message.data);
        }
        // Pause between down and up must still release the actual X11 inputs.
        let releases = vec![vec![4, 0, 0, 0, 0, 0, 0xff, 0xe1], vec![5, 0, 0, 4, 0, 5]];
        assert_eq!(input.releases(), releases);
        // An attempted up write can fail; keep its hold until success.
        for message in &messages[2..] {
            input.prepare_send(&message.data).unwrap();
        }
        assert_eq!(input.releases(), releases);
        for message in &messages[2..] {
            input.sent(&message.data);
        }
        assert!(input.releases().is_empty());
    }
}
