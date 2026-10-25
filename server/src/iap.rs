//! In-app purchases: coin packs bought in the iPhone app through Apple's App Store.
//!
//! The app hands the server a transaction id. The server never trusts the app about what was
//! bought: it asks Apple's App Store Server API for that transaction (over HTTPS, signed with
//! the developer's App Store Connect API key), checks it is a coin pack for this app that has
//! not been refunded, and only then adds coins, once per transaction. Refunds arrive as App
//! Store Server Notifications; the server looks the transaction up with Apple again and takes
//! the coins back.
//!
//! Turned on by setting `FT_APPLE_ISSUER_ID`, `FT_APPLE_KEY_ID`, `FT_APPLE_KEY_FILE` and
//! `FT_APPLE_BUNDLE_ID`. Without them purchases are off and the app does not show the packs.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Coin packs: App Store product id and coins. The price of each is set in App Store Connect.
pub const PACKS: [(&str, u32); 4] = [("coins_300", 300), ("coins_1000", 1000), ("coins_1800", 1800), ("coins_4000", 4000)];

const PRODUCTION: &str = "https://api.storekit.itunes.apple.com";
const SANDBOX: &str = "https://api.storekit-sandbox.itunes.apple.com";

pub fn pack_coins(product: &str) -> Option<u32> {
    PACKS.iter().find(|p| p.0 == product).map(|p| p.1)
}

pub struct AppleConfig {
    issuer: String,
    key_id: String,
    key: jsonwebtoken::EncodingKey,
    pub bundle: String,
    /// Accept sandbox purchases (TestFlight, App Review, sandbox testers). Apple reviews
    /// production builds with sandbox purchases, so this is on unless turned off.
    pub allow_sandbox: bool,
    /// Production and sandbox API addresses (replaced only in tests).
    bases: [String; 2],
}

impl AppleConfig {
    /// Read the settings; `Ok(None)` when purchases are not configured.
    pub fn from_env() -> Result<Option<Self>, String> {
        let get = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        let (Some(issuer), Some(key_id), Some(file), Some(bundle)) = (get("FT_APPLE_ISSUER_ID"), get("FT_APPLE_KEY_ID"), get("FT_APPLE_KEY_FILE"), get("FT_APPLE_BUNDLE_ID")) else {
            return Ok(None);
        };
        let pem = std::fs::read(&file).map_err(|e| format!("FT_APPLE_KEY_FILE {file}: {e}"))?;
        let key = jsonwebtoken::EncodingKey::from_ec_pem(&pem).map_err(|e| format!("FT_APPLE_KEY_FILE {file}: not an App Store Connect .p8 key ({e})"))?;
        let allow_sandbox = get("FT_APPLE_SANDBOX").is_none_or(|v| v != "0");
        // For testing against a stand-in for Apple only.
        let bases = match get("FT_APPLE_API_BASE") {
            Some(b) => [b.clone(), b],
            None => [PRODUCTION.into(), SANDBOX.into()],
        };
        Ok(Some(Self { issuer, key_id, key, bundle, allow_sandbox, bases }))
    }

    #[cfg(test)]
    pub fn for_test(pem: &[u8], bundle: &str, allow_sandbox: bool) -> Self {
        Self { issuer: "issuer".into(), key_id: "KEYID".into(), key: jsonwebtoken::EncodingKey::from_ec_pem(pem).unwrap(), bundle: bundle.into(), allow_sandbox, bases: [PRODUCTION.into(), SANDBOX.into()] }
    }

    #[cfg(test)]
    pub fn with_bases(mut self, production: &str, sandbox: &str) -> Self {
        self.bases = [production.into(), sandbox.into()];
        self
    }

    /// A short-lived token for the App Store Server API.
    fn token(&self) -> Result<String, String> {
        #[derive(Serialize)]
        struct Claims<'a> {
            iss: &'a str,
            iat: u64,
            exp: u64,
            aud: &'a str,
            bid: &'a str,
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        header.typ = Some("JWT".into());
        let claims = Claims { iss: &self.issuer, iat: now, exp: now + 20 * 60, aud: "appstoreconnect-v1", bid: &self.bundle };
        jsonwebtoken::encode(&header, &claims, &self.key).map_err(|e| e.to_string())
    }
}

/// The fields of Apple's `JWSTransactionDecodedPayload` that matter here.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppleTx {
    pub transaction_id: String,
    #[serde(default)]
    pub original_transaction_id: String,
    pub bundle_id: String,
    pub product_id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub quantity: Option<u32>,
    #[serde(default)]
    pub revocation_date: Option<u64>,
    #[serde(default)]
    pub environment: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IapError {
    /// Apple does not know this transaction.
    NotFound,
    /// Not a coin pack of this app (or a sandbox purchase while those are off).
    Invalid,
    /// Refunded or revoked.
    Revoked,
    /// Apple could not be reached or refused the request: try again later.
    Retry,
    /// The transaction was already used by another account.
    OtherAccount,
}

impl IapError {
    pub fn code(&self) -> &'static str {
        match self {
            IapError::NotFound => "iap_not_found",
            IapError::Invalid => "iap_invalid",
            IapError::Revoked => "iap_revoked",
            IapError::Retry => "iap_retry",
            IapError::OtherAccount => "iap_other_account",
        }
    }
}

/// The payload of a JWS (the middle part), decoded. Only used on data that came straight from
/// Apple over HTTPS, so the signature is not checked again here.
pub fn jws_payload<T: for<'de> Deserialize<'de>>(jws: &str) -> Option<T> {
    let mid = jws.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(mid.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// What a verified transaction is worth: (product, coins).
pub fn worth(cfg: &AppleConfig, tx: &AppleTx) -> Result<(String, u32), IapError> {
    if tx.bundle_id != cfg.bundle {
        return Err(IapError::Invalid);
    }
    if tx.environment == "Sandbox" && !cfg.allow_sandbox {
        return Err(IapError::Invalid);
    }
    if tx.revocation_date.is_some() {
        return Err(IapError::Revoked);
    }
    if !tx.kind.is_empty() && tx.kind != "Consumable" {
        return Err(IapError::Invalid);
    }
    let coins = pack_coins(&tx.product_id).ok_or(IapError::Invalid)?;
    let qty = tx.quantity.unwrap_or(1).clamp(1, 10);
    Ok((tx.product_id.clone(), coins * qty))
}

/// Look a transaction up with Apple: production first, then the sandbox (Apple's advice).
pub async fn fetch(cfg: &AppleConfig, http: &reqwest::Client, transaction_id: &str) -> Result<AppleTx, IapError> {
    if transaction_id.is_empty() || transaction_id.len() > 64 || !transaction_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(IapError::NotFound);
    }
    let token = cfg.token().map_err(|e| {
        tracing::error!(error = %e, "could not sign an App Store Server API token");
        IapError::Retry
    })?;
    let bases: &[String] = if cfg.allow_sandbox { &cfg.bases } else { &cfg.bases[..1] };
    for base in bases {
        let url = format!("{base}/inApps/v1/transactions/{transaction_id}");
        let res = http.get(&url).bearer_auth(&token).timeout(Duration::from_secs(10)).send().await.map_err(|e| {
            tracing::warn!(error = %e, "App Store Server API unreachable");
            IapError::Retry
        })?;
        match res.status().as_u16() {
            200 => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Body {
                    signed_transaction_info: String,
                }
                let body: Body = res.json().await.map_err(|_| IapError::Retry)?;
                return jws_payload::<AppleTx>(&body.signed_transaction_info).filter(|t| t.transaction_id == transaction_id).ok_or(IapError::Retry);
            }
            404 => continue,
            401 => {
                tracing::error!("App Store Server API refused the key: check FT_APPLE_ISSUER_ID, FT_APPLE_KEY_ID and the .p8 file");
                return Err(IapError::Retry);
            }
            s => {
                tracing::warn!(status = s, "App Store Server API error");
                return Err(IapError::Retry);
            }
        }
    }
    Err(IapError::NotFound)
}

/// The transaction id inside an App Store Server Notification (V2) body.
pub fn notification_tx(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Outer {
        signed_payload: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Payload {
        data: Data,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        signed_transaction_info: Option<String>,
    }
    let outer: Outer = serde_json::from_str(body).ok()?;
    let p: Payload = jws_payload(&outer.signed_payload)?;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Tx {
        transaction_id: String,
    }
    let tx: Tx = jws_payload(&p.data.signed_transaction_info?)?;
    Some(tx.transaction_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway P-256 key in the same PKCS#8 PEM form as an App Store Connect .p8 file.
    const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgaY6bLeWYYa2KprrM
lPIVHWOS0WALmKLwKtGHUr8rRz+hRANCAARgUe50Mqo72RuZKsOKNxsINVLwi8QL
KdcwDcMwIg/zmQzVuJnU5/Ug6+tfksvAFik9vaYyqKN2ZRy7ND+NpQhm
-----END PRIVATE KEY-----";

    fn b64(v: &serde_json::Value) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string())
    }

    fn jws(v: serde_json::Value) -> String {
        format!("{}.{}.sig", b64(&serde_json::json!({"alg":"ES256"})), b64(&v))
    }

    fn tx(product: &str) -> AppleTx {
        AppleTx {
            transaction_id: "2000000123".into(),
            original_transaction_id: "2000000123".into(),
            bundle_id: "com.fivetrails.app".into(),
            product_id: product.into(),
            kind: "Consumable".into(),
            quantity: Some(1),
            revocation_date: None,
            environment: "Production".into(),
        }
    }

    #[test]
    fn packs_are_unique_and_grow_in_value() {
        let mut ids: Vec<&str> = PACKS.iter().map(|p| p.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PACKS.len());
        assert!(PACKS.windows(2).all(|w| w[0].1 < w[1].1));
        assert_eq!(pack_coins("coins_1000"), Some(1000));
        assert_eq!(pack_coins("coins_9"), None);
    }

    #[test]
    fn only_a_live_coin_pack_of_this_app_is_worth_coins() {
        let cfg = AppleConfig::for_test(TEST_KEY.as_bytes(), "com.fivetrails.app", true);
        assert_eq!(worth(&cfg, &tx("coins_1800")), Ok(("coins_1800".into(), 1800)));
        let mut t = tx("coins_300");
        t.quantity = Some(3);
        assert_eq!(worth(&cfg, &t), Ok(("coins_300".into(), 900)));
        assert_eq!(worth(&cfg, &tx("gems_10")), Err(IapError::Invalid));
        let mut t = tx("coins_300");
        t.bundle_id = "com.someone.else".into();
        assert_eq!(worth(&cfg, &t), Err(IapError::Invalid), "another app's purchase");
        let mut t = tx("coins_300");
        t.revocation_date = Some(1);
        assert_eq!(worth(&cfg, &t), Err(IapError::Revoked));
        let mut t = tx("coins_300");
        t.environment = "Sandbox".into();
        assert!(worth(&cfg, &t).is_ok(), "sandbox purchases count while allowed (TestFlight, review)");
        let strict = AppleConfig::for_test(TEST_KEY.as_bytes(), "com.fivetrails.app", false);
        assert_eq!(worth(&strict, &t), Err(IapError::Invalid));
    }

    #[test]
    fn tokens_are_es256_with_the_key_id_and_apple_audience() {
        let cfg = AppleConfig::for_test(TEST_KEY.as_bytes(), "com.fivetrails.app", true);
        let t = cfg.token().unwrap();
        let header = jsonwebtoken::decode_header(&t).unwrap();
        assert_eq!((header.alg, header.kid.as_deref()), (jsonwebtoken::Algorithm::ES256, Some("KEYID")));
        let claims: serde_json::Value = jws_payload(&t).unwrap();
        assert_eq!((claims["aud"].as_str(), claims["bid"].as_str(), claims["iss"].as_str()), (Some("appstoreconnect-v1"), Some("com.fivetrails.app"), Some("issuer")));
        assert!(claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap() <= 3600, "Apple allows at most an hour");
    }

    /// A stand-in for Apple: the production API knows nothing, the sandbox knows one purchase.
    async fn fake_apple() -> String {
        use axum::{extract::Path, http::HeaderMap, routing::get, Router};
        let app = Router::new()
            .route("/prod/inApps/v1/transactions/{id}", get(|| async { axum::http::StatusCode::NOT_FOUND }))
            .route(
                "/sbx/inApps/v1/transactions/{id}",
                get(|Path(id): Path<String>, h: HeaderMap| async move {
                    let auth = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
                    if !auth.starts_with("Bearer ey") {
                        return (axum::http::StatusCode::UNAUTHORIZED, String::new());
                    }
                    if id != "2000000123" {
                        return (axum::http::StatusCode::NOT_FOUND, String::new());
                    }
                    let info = jws(serde_json::json!({ "transactionId": id, "bundleId": "com.fivetrails.app", "productId": "coins_1000", "type": "Consumable", "quantity": 1, "environment": "Sandbox" }));
                    (axum::http::StatusCode::OK, serde_json::json!({ "signedTransactionInfo": info }).to_string())
                }),
            );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn purchases_are_looked_up_with_apple_production_first_then_sandbox() {
        let base = fake_apple().await;
        let http = reqwest::Client::new();
        let cfg = AppleConfig::for_test(TEST_KEY.as_bytes(), "com.fivetrails.app", true).with_bases(&format!("{base}/prod"), &format!("{base}/sbx"));
        let t = fetch(&cfg, &http, "2000000123").await.unwrap();
        assert_eq!((t.product_id.as_str(), t.environment.as_str()), ("coins_1000", "Sandbox"));
        assert_eq!(worth(&cfg, &t), Ok(("coins_1000".into(), 1000)));
        assert_eq!(fetch(&cfg, &http, "2000000999").await, Err(IapError::NotFound));
        assert_eq!(fetch(&cfg, &http, "../../etc").await, Err(IapError::NotFound), "odd ids never reach Apple");
        let strict = AppleConfig::for_test(TEST_KEY.as_bytes(), "com.fivetrails.app", false).with_bases(&format!("{base}/prod"), &format!("{base}/sbx"));
        assert_eq!(fetch(&strict, &http, "2000000123").await, Err(IapError::NotFound), "sandbox not asked when off");
        let down = AppleConfig::for_test(TEST_KEY.as_bytes(), "com.fivetrails.app", true).with_bases("http://127.0.0.1:9", "http://127.0.0.1:9");
        assert_eq!(fetch(&down, &http, "2000000123").await, Err(IapError::Retry), "Apple unreachable: try later");
    }

    #[test]
    fn notifications_yield_the_transaction_id() {
        let info = jws(serde_json::json!({ "transactionId": "2000000999", "bundleId": "com.fivetrails.app", "productId": "coins_300" }));
        let payload = jws(serde_json::json!({ "notificationType": "REFUND", "data": { "signedTransactionInfo": info } }));
        let body = serde_json::json!({ "signedPayload": payload }).to_string();
        assert_eq!(notification_tx(&body).as_deref(), Some("2000000999"));
        assert_eq!(notification_tx("{}"), None);
        assert_eq!(notification_tx(r#"{"signedPayload":"x.y.z"}"#), None);
    }
}
