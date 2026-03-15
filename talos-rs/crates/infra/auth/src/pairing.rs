//! Device Pairing Implementation
//!
//! Implements device pairing flow similar to OpenClaw's pairing mechanism.

use talos_error::{TalosError, TalosResult};
use tracing::{debug, info};

/// Pairing result
#[derive(Debug, Clone)]
pub struct PairingResult {
    pub device_id: String,
    pub device_token: String,
    pub is_approved: bool,
}

/// Pairing code for device authentication
#[derive(Debug, Clone)]
pub struct PairingCode {
    pub code: String,
    pub device_id: String,
    pub expires_at: u64,
}

impl PairingCode {
    /// Create a new pairing code
    pub fn new(device_id: &str, code: &str, validity_secs: u64) -> Self {
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + validity_secs;

        Self {
            code: code.to_string(),
            device_id: device_id.to_string(),
            expires_at,
        }
    }

    /// Check if the pairing code is expired
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now > self.expires_at
    }
}

/// Device Pairing manager
pub struct DevicePairing {
    /// Pending pairing codes (in production, use Redis/database)
    pending_codes: std::collections::HashMap<String, PairingCode>,
    /// Approved devices
    approved_devices: std::collections::HashMap<String, String>, // device_id -> device_token
}

impl Default for DevicePairing {
    fn default() -> Self {
        Self::new()
    }
}

impl DevicePairing {
    /// Create a new device pairing manager
    pub fn new() -> Self {
        info!("Device pairing manager initialized");

        Self {
            pending_codes: std::collections::HashMap::new(),
            approved_devices: std::collections::HashMap::new(),
        }
    }

    /// Generate a new pairing code for a device
    pub fn generate_code(&mut self, device_id: &str) -> PairingCode {
        // Generate 6-digit code
        let code = format!("{:06}", rand::random::<u32>() % 1000000);

        let pairing_code = PairingCode::new(device_id, &code, 300); // 5 minutes validity

        debug!("Generated pairing code for device: {}", device_id);

        self.pending_codes
            .insert(device_id.to_string(), pairing_code.clone());

        pairing_code
    }

    /// Verify a pairing code
    pub fn verify_code(&mut self, device_id: &str, code: &str) -> TalosResult<PairingResult> {
        debug!("Verifying pairing code for device: {}", device_id);

        let pairing_code = self
            .pending_codes
            .get(device_id)
            .ok_or_else(|| TalosError::AuthError("Device not found".to_string()))?;

        if pairing_code.is_expired() {
            self.pending_codes.remove(device_id);
            return Err(TalosError::AuthError("Pairing code expired".to_string()));
        }

        if pairing_code.code != code {
            return Err(TalosError::AuthError("Invalid pairing code".to_string()));
        }

        // Code is valid, approve the device
        self.pending_codes.remove(device_id);

        // Generate device token
        let device_token = uuid::Uuid::new_v4().to_string();
        self.approved_devices
            .insert(device_id.to_string(), device_token.clone());

        info!("Device approved: {}", device_id);

        Ok(PairingResult {
            device_id: device_id.to_string(),
            device_token,
            is_approved: true,
        })
    }

    /// Check if a device is approved
    pub fn is_approved(&self, device_id: &str) -> bool {
        self.approved_devices.contains_key(device_id)
    }

    /// Get device token
    pub fn get_device_token(&self, device_id: &str) -> Option<String> {
        self.approved_devices.get(device_id).cloned()
    }

    /// Revoke a device
    pub fn revoke(&mut self, device_id: &str) -> TalosResult<()> {
        debug!("Revoking device: {}", device_id);

        self.approved_devices
            .remove(device_id)
            .ok_or_else(|| TalosError::AuthError("Device not found".to_string()))?;

        info!("Device revoked: {}", device_id);

        Ok(())
    }

    /// List all approved devices
    pub fn list_devices(&self) -> Vec<String> {
        self.approved_devices.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pairing_code_expiry() {
        let code = PairingCode::new("device-123", "123456", 300);
        assert!(!code.is_expired());
    }

    #[test]
    fn test_device_pairing() {
        let mut pairing = DevicePairing::new();

        // Generate code
        let generated = pairing.generate_code("device-123");
        assert_eq!(generated.code.len(), 6);

        // Verify code
        let result = pairing.verify_code("device-123", &generated.code).unwrap();
        assert!(result.is_approved);
        assert!(!result.device_token.is_empty());

        // Check if approved
        assert!(pairing.is_approved("device-123"));
    }

    #[test]
    fn test_invalid_pairing_code() {
        let mut pairing = DevicePairing::new();
        pairing.generate_code("device-123");

        // Wrong code
        let result = pairing.verify_code("device-123", "wrong-code");
        assert!(result.is_err());
    }

    #[test]
    fn test_device_revoke() {
        let mut pairing = DevicePairing::new();
        let generated = pairing.generate_code("device-123");
        pairing.verify_code("device-123", &generated.code).unwrap();

        // Revoke
        pairing.revoke("device-123").unwrap();
        assert!(!pairing.is_approved("device-123"));
    }
}
