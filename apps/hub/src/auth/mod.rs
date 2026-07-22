//! Pairing auth with MongoDB persistence.
//!
//! `POST /api/v1/auth/pair` issues an opaque Bearer token stored in MongoDB
//! (`pairing_codes`, `tokens`). When `AUTH_REQUIRED=false` (default), middleware
//! is a no-op so demos work without a token. When Mongo is down and auth is
//! required, protected routes return 503.

mod middleware;

pub use middleware::require_bearer;

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use mongodb::bson::{doc, DateTime as BsonDateTime};
use mongodb::options::IndexOptions;
use mongodb::{Collection, IndexModel};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::mongo::MongoStore;
use crate::AppState;

static TOKEN_SEQ: AtomicU64 = AtomicU64::new(1);
static CODE_SEQ: AtomicU64 = AtomicU64::new(1);

const PAIRING_TTL: Duration = Duration::from_secs(10 * 60);
const TOKEN_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PairingCodeDoc {
    code: String,
    expires_at: BsonDateTime,
    created_at: BsonDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenDoc {
    token: String,
    code: String,
    expires_at: BsonDateTime,
    created_at: BsonDateTime,
}

struct AuthDb {
    codes: Collection<PairingCodeDoc>,
    tokens: Collection<TokenDoc>,
}

/// Pairing codes + issued tokens backed by MongoDB when available.
pub struct AuthService {
    secret: String,
    db: Option<AuthDb>,
    ready: AtomicBool,
}

impl AuthService {
    /// Bind to a shared [`MongoStore`], ensure indexes, and return a service.
    /// When `store` is `None` or index setup fails: `available() == false`.
    pub async fn from_store(store: Option<&MongoStore>, secret: impl Into<String>) -> Self {
        let secret = secret.into();
        let Some(store) = store else {
            return Self {
                secret,
                db: None,
                ready: AtomicBool::new(false),
            };
        };

        match Self::setup(store).await {
            Ok(db) => {
                tracing::info!(
                    database = store.database_name(),
                    "MongoDB auth store ready (pairing_codes, tokens)"
                );
                Self {
                    secret,
                    db: Some(db),
                    ready: AtomicBool::new(true),
                }
            }
            Err(e) => {
                tracing::error!(
                    database = store.database_name(),
                    error = %e,
                    "MongoDB auth indexes failed — auth persistence disabled"
                );
                Self {
                    secret,
                    db: None,
                    ready: AtomicBool::new(false),
                }
            }
        }
    }

    async fn setup(store: &MongoStore) -> Result<AuthDb, mongodb::error::Error> {
        let codes: Collection<PairingCodeDoc> = store.collection("pairing_codes");
        let tokens: Collection<TokenDoc> = store.collection("tokens");
        ensure_indexes(&codes, &tokens).await?;
        Ok(AuthDb { codes, tokens })
    }

    /// True when MongoDB was reached at startup and indexes are in place.
    pub fn available(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }

    /// Pair: accept an existing code, or generate one. Always issues a token.
    pub async fn pair(&self, code: Option<String>) -> Result<PairResponse, PairError> {
        let db = self.db.as_ref().ok_or(PairError::Unavailable)?;

        let code = match code {
            Some(c) => {
                let c = c.trim().to_uppercase();
                if c.is_empty() {
                    return Err(PairError::InvalidCode);
                }
                self.consume_or_accept_code(db, &c).await?;
                c
            }
            None => self.mint_pairing_code(db).await?,
        };

        let token = self.issue_token(db, &code).await?;
        Ok(PairResponse {
            code,
            token,
            token_type: "Bearer",
            expires_in: TOKEN_TTL_SECS,
        })
    }

    pub async fn validate_token(&self, token: &str) -> Result<bool, PairError> {
        let db = self.db.as_ref().ok_or(PairError::Unavailable)?;
        let now = BsonDateTime::now();
        match db
            .tokens
            .find_one(doc! {
                "token": token,
                "expires_at": { "$gt": now },
            })
            .await
        {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => {
                tracing::error!(error = %e, "MongoDB token lookup failed");
                self.mark_down();
                Err(PairError::Unavailable)
            }
        }
    }

    /// Revoke (delete) a Bearer token — used by logout.
    pub async fn revoke_token(&self, token: &str) -> Result<bool, PairError> {
        let db = self.db.as_ref().ok_or(PairError::Unavailable)?;
        let token = token.trim();
        if token.is_empty() {
            return Ok(false);
        }
        match db.tokens.delete_one(doc! { "token": token }).await {
            Ok(res) => Ok(res.deleted_count > 0),
            Err(e) => {
                tracing::error!(error = %e, "MongoDB token revoke failed");
                self.mark_down();
                Err(PairError::Unavailable)
            }
        }
    }

    async fn mint_pairing_code(&self, db: &AuthDb) -> Result<String, PairError> {
        let n = CODE_SEQ.fetch_add(1, Ordering::Relaxed);
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // 6-char uppercase hex-ish stub code.
        let code = format!("{:04X}{:02X}", (t ^ n) as u16, (n & 0xFF) as u8);
        let now = BsonDateTime::now();
        let expires_at = bson_after(PAIRING_TTL);
        let doc = PairingCodeDoc {
            code: code.clone(),
            expires_at,
            created_at: now,
        };
        db.codes
            .replace_one(doc! { "code": &code }, doc)
            .upsert(true)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "MongoDB pairing code insert failed");
                self.mark_down();
                PairError::Unavailable
            })?;
        Ok(code)
    }

    async fn consume_or_accept_code(&self, db: &AuthDb, code: &str) -> Result<(), PairError> {
        let now = BsonDateTime::now();

        match db.codes.find_one(doc! { "code": code }).await {
            Ok(Some(existing)) => {
                if existing.expires_at <= now {
                    let _ = db.codes.delete_one(doc! { "code": code }).await;
                    return Err(PairError::ExpiredCode);
                }
                // Consume: one-shot redeem.
                let _ = db.codes.delete_one(doc! { "code": code }).await;
                Ok(())
            }
            Ok(None) => {
                // Stub convenience: unknown codes are accepted once and remembered briefly
                // so a UI can type a shared LAN code without a separate mint step.
                let doc = PairingCodeDoc {
                    code: code.to_string(),
                    expires_at: bson_after(PAIRING_TTL),
                    created_at: now,
                };
                db.codes
                    .replace_one(doc! { "code": code }, doc)
                    .upsert(true)
                    .await
                    .map_err(|e| {
                        tracing::error!(error = %e, "MongoDB pairing code accept failed");
                        self.mark_down();
                        PairError::Unavailable
                    })?;
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "MongoDB pairing code lookup failed");
                self.mark_down();
                Err(PairError::Unavailable)
            }
        }
    }

    async fn issue_token(&self, db: &AuthDb, code: &str) -> Result<String, PairError> {
        let n = TOKEN_SEQ.fetch_add(1, Ordering::Relaxed);
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let token = if self.secret.is_empty() {
            format!("lit_{t:x}_{n:x}")
        } else {
            // Lightweight stub "HMAC" (not cryptographic) — binds secret + code.
            let mut h: u64 = 0xcbf29ce484222325;
            for b in format!("{t}:{n}:{code}:{}", self.secret).bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x100000001b3);
            }
            format!("lit_{h:016x}_{n:x}")
        };

        let now = BsonDateTime::now();
        let doc = TokenDoc {
            token: token.clone(),
            code: code.to_string(),
            expires_at: bson_after(Duration::from_secs(TOKEN_TTL_SECS)),
            created_at: now,
        };
        db.tokens
            .replace_one(doc! { "token": &token }, doc)
            .upsert(true)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "MongoDB token insert failed");
                self.mark_down();
                PairError::Unavailable
            })?;
        Ok(token)
    }

    fn mark_down(&self) {
        self.ready.store(false, Ordering::Relaxed);
    }
}

async fn ensure_indexes(
    codes: &Collection<PairingCodeDoc>,
    tokens: &Collection<TokenDoc>,
) -> Result<(), mongodb::error::Error> {
    let mut unique_opts = IndexOptions::default();
    unique_opts.unique = Some(true);

    let mut ttl_opts = IndexOptions::default();
    ttl_opts.expire_after = Some(Duration::from_secs(0));

    let code_unique = IndexModel::builder()
        .keys(doc! { "code": 1 })
        .options(Some(unique_opts.clone()))
        .build();
    let code_ttl = IndexModel::builder()
        .keys(doc! { "expires_at": 1 })
        .options(Some(ttl_opts.clone()))
        .build();
    codes.create_indexes(vec![code_unique, code_ttl]).await?;

    let token_unique = IndexModel::builder()
        .keys(doc! { "token": 1 })
        .options(Some(unique_opts))
        .build();
    let token_ttl = IndexModel::builder()
        .keys(doc! { "expires_at": 1 })
        .options(Some(ttl_opts))
        .build();
    tokens.create_indexes(vec![token_unique, token_ttl]).await?;

    Ok(())
}

fn bson_after(d: Duration) -> BsonDateTime {
    let millis = BsonDateTime::now().timestamp_millis() + d.as_millis() as i64;
    BsonDateTime::from_millis(millis)
}

#[derive(Debug)]
pub enum PairError {
    InvalidCode,
    ExpiredCode,
    Unavailable,
}

#[derive(Debug, Deserialize)]
pub struct PairRequest {
    /// Optional pairing code. Omit (or null) to mint a new code + token.
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PairResponse {
    pub code: String,
    pub token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/auth/pair", post(pair))
        .route("/api/v1/auth/logout", post(logout))
}

/// POST /api/v1/auth/pair — `{ "code"?: "ABC123" }` → opaque Bearer token.
async fn pair(
    State(state): State<Arc<AppState>>,
    body: Option<Json<PairRequest>>,
) -> Result<Json<PairResponse>, (StatusCode, Json<ErrorBody>)> {
    let code = body.and_then(|Json(b)| b.code);
    match state.auth.pair(code).await {
        Ok(resp) => Ok(Json(resp)),
        Err(PairError::InvalidCode) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "invalid pairing code".into(),
            }),
        )),
        Err(PairError::ExpiredCode) => Err((
            StatusCode::GONE,
            Json(ErrorBody {
                error: "pairing code expired".into(),
            }),
        )),
        Err(PairError::Unavailable) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "auth store unavailable (MongoDB)".into(),
            }),
        )),
    }
}

#[derive(Debug, Deserialize)]
struct LogoutRequest {
    /// Optional; if omitted, Authorization Bearer is used.
    #[serde(default)]
    token: Option<String>,
}

/// POST /api/v1/auth/logout — revoke token (body.token or Authorization Bearer).
async fn logout(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    body: Option<Json<LogoutRequest>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    let from_body = body.and_then(|Json(b)| b.token).filter(|t| !t.trim().is_empty());
    let from_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
                .map(|t| t.trim().to_string())
        })
        .filter(|t| !t.is_empty());
    let Some(token) = from_body.or(from_header) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "token required (body.token or Authorization Bearer)".into(),
            }),
        ));
    };
    match state.auth.revoke_token(&token).await {
        Ok(deleted) => Ok(Json(serde_json::json!({ "ok": true, "revoked": deleted }))),
        Err(PairError::Unavailable) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "auth store unavailable (MongoDB)".into(),
            }),
        )),
        Err(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "logout failed".into(),
            }),
        )),
    }
}
