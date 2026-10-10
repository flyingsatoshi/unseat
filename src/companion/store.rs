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
    pub fn from_json(bytes: &[u8]) -> Self {
        let mut s: Store = serde_json::from_slice(bytes).unwrap_or_default();
        s.version = STORE_VERSION;
        s.devices.truncate(MAX_DEVICES);
        s
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
