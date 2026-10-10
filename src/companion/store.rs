//! Paired devices, kept in `%APPDATA%\Unseat\companion.json`, apart from settings.json so the
//! settings window and "reset to defaults" never touch them. No device secret is stored, only
//! its hash; the VAPID private key is stored encrypted by the host (DPAPI).

use serde::{Deserialize, Serialize};

pub const STORE_VERSION: u32 = 1;
pub const MAX_DEVICES: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// Tailscale login seen at pairing. Later requests must come from the same login.
    pub login: String,
    pub token_hash: String,
    pub created_ms: u64,
    #[serde(default)]
    pub last_seen_ms: u64,
    #[serde(default)]
    pub subscription: Option<Subscription>,
    /// Apple said the subscription is gone (404/410). Cleared when the phone subscribes again.
    #[serde(default)]
    pub needs_resubscribe: bool,
    #[serde(default)]
    pub delivered: u64,
    #[serde(default)]
    pub missed: u64,
    #[serde(default)]
    pub last_delivered_ms: u64,
    /// Reminders the phone itself confirmed (its service worker saw the push).
    #[serde(default)]
    pub received: u64,
    #[serde(default)]
    pub last_received_ms: u64,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl Device {
    pub fn can_receive(&self) -> bool {
        self.subscription.is_some() && !self.needs_resubscribe
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub devices: Vec<Device>,
    /// The VAPID private key, encrypted by the host for the current Windows user, base64.
    #[serde(default)]
    pub vapid_protected: Option<String>,
}

impl Store {
    /// Read a saved store. An error means the file is damaged: the host keeps it aside
    /// instead of overwriting it, so a bad read never silently unpairs every phone.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut s: Store = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        s.version = STORE_VERSION;
        s.devices.truncate(MAX_DEVICES);
        Ok(s)
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).unwrap_or_else(|_| b"{}".to_vec())
    }

    pub fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn device_mut(&mut self, id: &str) -> Option<&mut Device> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    /// Devices that can receive a push now.
    pub fn receivers(&self) -> Vec<String> {
        self.devices.iter().filter(|d| d.can_receive()).map(|d| d.id.clone()).collect()
    }
}
