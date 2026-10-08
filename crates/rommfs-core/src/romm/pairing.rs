//! RomM device pairing at the source revision recorded in API-CONTRACT.md.
//! Client tokens have optional expiry but no refresh token; re-pair on rejection.
use super::client::{transport, RommClient};
use crate::{Error, Result};
use isahc::prelude::*;
use serde::{Deserialize, Serialize};

// Secrets intentionally do not implement Debug.
#[derive(Deserialize)]
pub struct PairingRequest {
    pub device_code: String,
    pub user_code: String,
    pub verification_path: String,
    pub verification_path_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PairedSession {
    pub access_token: String,
    pub device_id: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<String>,
}

pub enum PairingPoll {
    Pending,
    SlowDown,
    Denied,
    Expired,
    Approved(PairedSession),
}

impl RommClient {
    pub fn begin_pairing(&self, device_identifier: &str) -> Result<PairingRequest> {
        let payload = serde_json::json!({
            "client_device_identifier": device_identifier, "name": "RomMFS",
            "client": "rommfs", "platform": "windows", "client_version": env!("CARGO_PKG_VERSION"),
            "requested_scopes": ["platforms.read", "roms.read", "me.read", "assets.read", "assets.write", "firmware.read"]
        });
        let mut response = self.pairing_post("/api/auth/device/init", payload)?;
        match response.status().as_u16() {
            201 => response.json().map_err(|e| Error::InvalidCatalogue(format!("pairing response: {e}"))),
            404 => Err(Error::Unsupported("server lacks /api/auth/device/init; use password sign-in or upgrade to a server with device pairing".into())),
            status => Err(Error::Http { status, message: "device pairing could not start".into() }),
        }
    }

    pub fn pairing_url(&self, request: &PairingRequest) -> Result<String> {
        let path = &request.verification_path_complete;
        // Never let a remote pairing payload launch another origin/scheme.
        if !path.starts_with("/pair/device?") || path.contains(['\\', '\r', '\n', '#']) {
            return Err(Error::InvalidCatalogue(
                "invalid pairing verification path".into(),
            ));
        }
        Ok(format!("{}{path}", self.base_url))
    }

    pub fn poll_pairing(&self, device_code: &str) -> Result<PairingPoll> {
        let mut response = self.pairing_post(
            "/api/auth/device/token",
            serde_json::json!({"device_code": device_code}),
        )?;
        match response.status().as_u16() {
            200 => response
                .json()
                .map(PairingPoll::Approved)
                .map_err(|_| Error::Auth("invalid paired token response; sign in again".into())),
            400 => {
                #[derive(Deserialize)]
                struct Detail {
                    detail: String,
                }
                let detail: Detail = response.json().map_err(|_| {
                    Error::InvalidCatalogue("invalid pairing status response".into())
                })?;
                match detail.detail.as_str() {
                    "authorization_pending" => Ok(PairingPoll::Pending),
                    "slow_down" => Ok(PairingPoll::SlowDown),
                    "access_denied" => Ok(PairingPoll::Denied),
                    "expired_token" => Ok(PairingPoll::Expired),
                    _ => Err(Error::InvalidCatalogue("unknown pairing status".into())),
                }
            }
            429 => Ok(PairingPoll::SlowDown),
            status => Err(Error::Http {
                status,
                message: "device pairing poll failed".into(),
            }),
        }
    }

    /// Restoration must still verify /api/users/me before using cached metadata.
    pub fn restore_paired_session(&self, session: &PairedSession) -> Result<()> {
        if session.access_token.is_empty() {
            return Err(Error::Auth("empty paired session".into()));
        }
        *self.token.write().expect("token lock poisoned") = Some(session.access_token.clone());
        Ok(())
    }

    fn pairing_post(
        &self,
        path: &str,
        payload: serde_json::Value,
    ) -> Result<isahc::Response<isahc::Body>> {
        isahc::Request::post(format!("{}{path}", self.base_url))
            .header("Content-Type", "application/json")
            .connect_timeout(self.metadata.connect_timeout)
            .timeout(self.metadata.response_timeout)
            .body(payload.to_string())
            .map_err(|_| Error::Transport("could not build device pairing request".into()))?
            .send()
            .map_err(transport)
    }
}
