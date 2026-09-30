//! Authentication for the HTTP API: web logins (cookie sessions), bearer tokens,
//! and the per-route access policy applied by the router.

mod handlers;

use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::sync::{Mutex, OnceLock};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use musicd_core::AuthMode;
use sha2::{Digest, Sha256};

use crate::db::Database;
use crate::http::{HttpRequest, ResponseWriter, api_error, write_response_owned};
use crate::service::ServiceState;
use crate::util::{now_unix_timestamp, url_encode};

pub(crate) use handlers::{
    handle_api_tokens_create_request, handle_api_tokens_list_request,
    handle_api_tokens_revoke_request, handle_change_password_form_request,
    handle_change_password_request, handle_login_form_request, handle_login_request,
    handle_logout_request,
};

pub(crate) const SESSION_COOKIE: &str = "musicd_session";
const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// Session expiry and token `last_used` are only rewritten this often.
const TOUCH_INTERVAL_SECS: i64 = 60 * 60;
const TOKEN_TOUCH_INTERVAL_SECS: i64 = 60;

pub(crate) const DEFAULT_ADMIN_USERNAME: &str = "admin";
const DEFAULT_ADMIN_PASSWORD: &str = "password";
pub(crate) const MIN_PASSWORD_LENGTH: usize = 8;

const API_TOKEN_PREFIX: &str = "mdt_";

const LOGIN_FAILURE_LIMIT: u32 = 5;
const LOGIN_FAILURE_WINDOW_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenScope {
    /// Everything, including admin-only routes.
    Admin,
    /// Everything a controller app needs; no admin-only routes.
    Client,
}

impl TokenScope {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Client => "client",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "admin" => Some(Self::Admin),
            "client" => Some(Self::Client),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Principal {
    /// A browser signed in with a username and password. Web users are admins.
    User {
        user_id: i64,
        username: String,
        must_change_password: bool,
        session_hash: String,
    },
    Token {
        id: String,
        name: String,
        scope: TokenScope,
    },
}

impl Principal {
    fn is_admin(&self) -> bool {
        match self {
            Self::User { .. } => true,
            Self::Token { scope, .. } => *scope == TokenScope::Admin,
        }
    }

    fn must_change_password(&self) -> bool {
        matches!(
            self,
            Self::User {
                must_change_password: true,
                ..
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Credentials {
    Anonymous,
    Valid(Principal),
    /// A bearer token was sent but is unknown or revoked.
    InvalidBearer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteAccess {
    /// Never needs credentials (renderer fetches, discovery, login pages, static assets).
    Public,
    /// Needs credentials only when auth is `required`.
    Standard,
    /// Always needs a web login or an admin token.
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow,
    Unauthorized,
    Forbidden,
    PasswordChangeRequired,
}

pub(crate) fn route_access(method: &str, path: &str) -> RouteAccess {
    const PUBLIC_PREFIXES: &[&str] = &["/assets/", "/stream/", "/artwork/"];
    const PUBLIC_ROUTES: &[&str] = &[
        "/health",
        "/description.xml",
        "/metrics",
        "/login",
        "/logout",
        "/account/password",
    ];
    // Starting a library scan, linking Tidal, bulk recommendation changes and
    // credential management are admin-only.
    const ADMIN_ROUTES: &[&str] = &[
        "/rescan",
        "/rescan-progress",
        "/api/tidal/auth-url",
        "/api/tidal/complete-auth",
        "/api/recommendations/import",
    ];

    if PUBLIC_ROUTES.contains(&path) || PUBLIC_PREFIXES.iter().any(|p| path.starts_with(p)) {
        return RouteAccess::Public;
    }
    if ADMIN_ROUTES.contains(&path)
        || path == "/api/auth/tokens"
        || path.starts_with("/api/auth/tokens/")
        || (method == "DELETE" && path == "/api/recommendations")
    {
        return RouteAccess::Admin;
    }
    RouteAccess::Standard
}

pub(crate) fn decide(mode: AuthMode, access: RouteAccess, credentials: &Credentials) -> Decision {
    if mode == AuthMode::Off || access == RouteAccess::Public {
        return Decision::Allow;
    }
    let principal = match credentials {
        Credentials::InvalidBearer => return Decision::Unauthorized,
        Credentials::Anonymous => {
            return if access == RouteAccess::Standard && mode == AuthMode::Optional {
                Decision::Allow
            } else {
                Decision::Unauthorized
            };
        }
        Credentials::Valid(principal) => principal,
    };
    if principal.must_change_password() {
        return Decision::PasswordChangeRequired;
    }
    if access == RouteAccess::Admin && !principal.is_admin() {
        return Decision::Forbidden;
    }
    Decision::Allow
}

/// Applies the access policy to a request. Returns false after writing a
/// 401/403/redirect response, in which case the caller must stop.
pub(crate) fn authorize_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<bool> {
    let mode = state.config.auth_mode;
    let access = route_access(&request.method, &request.path);
    if mode == AuthMode::Off || access == RouteAccess::Public {
        return Ok(true);
    }
    let credentials = authenticate(&state.database, request)?;
    let wants_page = wants_html_response(request);
    match decide(mode, access, &credentials) {
        Decision::Allow => Ok(true),
        Decision::Unauthorized if wants_page => {
            redirect(
                writer,
                &format!("/login?next={}", url_encode(&next_target(request))),
            )?;
            Ok(false)
        }
        Decision::Unauthorized => {
            let message = if credentials == Credentials::InvalidBearer {
                "invalid or revoked token"
            } else {
                "authentication required"
            };
            write_response_owned(
                writer,
                "401 Unauthorized",
                &[
                    (
                        "Content-Type".to_string(),
                        "application/json; charset=utf-8".to_string(),
                    ),
                    ("WWW-Authenticate".to_string(), "Bearer".to_string()),
                ],
                Some(format!(r#"{{"ok":false,"error":"{message}"}}"#).as_bytes()),
            )?;
            Ok(false)
        }
        Decision::PasswordChangeRequired if wants_page => {
            redirect(writer, "/account/password")?;
            Ok(false)
        }
        Decision::PasswordChangeRequired => {
            api_error(writer, "403 Forbidden", "password change required")?;
            Ok(false)
        }
        Decision::Forbidden => {
            api_error(writer, "403 Forbidden", "admin access required")?;
            Ok(false)
        }
    }
}

/// Browser page loads and form posts get redirected to the login page; API
/// and script clients get a status code instead.
fn wants_html_response(request: &HttpRequest) -> bool {
    const DATA_ROUTES: &[&str] = &["/mcp", "/library/rows", "/queue/panel", "/rescan-progress"];
    !request.path.starts_with("/api/") && !DATA_ROUTES.contains(&request.path.as_str())
}

/// Where to send the browser after it signs in. Form posts go back to the
/// page that posted them rather than replaying the action.
fn next_target(request: &HttpRequest) -> String {
    if request.method == "GET" {
        request.target.clone()
    } else {
        crate::http::request_value(request, "return_to")
            .filter(|value| is_safe_redirect_target(value))
            .unwrap_or("/")
            .to_string()
    }
}

/// Only same-origin absolute paths; rejects `//host` and `/\host` tricks.
pub(crate) fn is_safe_redirect_target(target: &str) -> bool {
    target.starts_with('/')
        && !target.starts_with("//")
        && !target.starts_with("/\\")
        && !target.chars().any(char::is_control)
}

pub(crate) fn authenticate(database: &Database, request: &HttpRequest) -> io::Result<Credentials> {
    let now = now_unix_timestamp();
    if let Some(token) = request.authorization.as_deref().and_then(bearer_token) {
        let Some(record) = database.load_active_api_token(&hash_secret(token))? else {
            return Ok(Credentials::InvalidBearer);
        };
        let Some(scope) = TokenScope::parse(&record.scope) else {
            return Ok(Credentials::InvalidBearer);
        };
        if record
            .last_used_unix
            .is_none_or(|last| now - last >= TOKEN_TOUCH_INTERVAL_SECS)
        {
            database.touch_api_token(&record.id, now)?;
        }
        return Ok(Credentials::Valid(Principal::Token {
            id: record.id,
            name: record.name,
            scope,
        }));
    }

    if let Some(secret) = request
        .cookie
        .as_deref()
        .and_then(|header| cookie_value(header, SESSION_COOKIE))
    {
        let session_hash = hash_secret(secret);
        if let Some(session) = database.load_web_session(&session_hash, now)? {
            if now - session.last_seen_unix >= TOUCH_INTERVAL_SECS {
                database.touch_web_session(&session_hash, now, now + SESSION_TTL_SECS)?;
            }
            return Ok(Credentials::Valid(Principal::User {
                user_id: session.user_id,
                username: session.username,
                must_change_password: session.must_change_password,
                session_hash,
            }));
        }
    }
    Ok(Credentials::Anonymous)
}

/// The signed-in web user's name, for page chrome. Tokens and anonymous
/// requests yield `None`.
pub(crate) fn signed_in_username(state: &ServiceState, request: &HttpRequest) -> Option<String> {
    match authenticate(&state.database, request) {
        Ok(Credentials::Valid(Principal::User { username, .. })) => Some(username),
        _ => None,
    }
}

fn bearer_token(header: &str) -> Option<&str> {
    let (scheme, token) = header.trim().split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("Bearer") && !token.is_empty()).then_some(token)
}

pub(crate) fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == name && !value.is_empty()).then_some(value)
    })
}

pub(crate) fn session_cookie(secret: &str) -> String {
    format!(
        "{SESSION_COOKIE}={secret}; Path=/; HttpOnly; SameSite=Strict; Max-Age={SESSION_TTL_SECS}"
    )
}

pub(crate) fn cleared_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

/// Creates a web session for the user and returns the cookie secret.
pub(crate) fn start_web_session(database: &Database, user_id: i64) -> io::Result<String> {
    let secret = random_secret()?;
    let now = now_unix_timestamp();
    database.insert_web_session(&hash_secret(&secret), user_id, now, now + SESSION_TTL_SECS)?;
    Ok(secret)
}

/// Mints an API token. The secret is returned once and only its hash is stored.
pub(crate) fn create_api_token(
    database: &Database,
    name: &str,
    scope: TokenScope,
) -> io::Result<(crate::db::ApiTokenRecord, String)> {
    let secret = format!("{API_TOKEN_PREFIX}{}", random_secret()?);
    let id = random_hex(8)?;
    let record = database.insert_api_token(
        &id,
        name,
        &hash_secret(&secret),
        scope.label(),
        now_unix_timestamp(),
    )?;
    Ok((record, secret))
}

/// Seeds `admin`/`password` on first start; the first login must change it.
pub(crate) fn ensure_default_admin(database: &Database) -> io::Result<()> {
    if database.count_users()? > 0 {
        return Ok(());
    }
    let hash = hash_password(DEFAULT_ADMIN_PASSWORD)?;
    database.insert_user(DEFAULT_ADMIN_USERNAME, &hash, true, now_unix_timestamp())?;
    eprintln!(
        "auth: created web user '{DEFAULT_ADMIN_USERNAME}' with the default password; \
         it must be changed at first sign-in"
    );
    Ok(())
}

pub(crate) fn hash_password(password: &str) -> io::Result<String> {
    let mut salt = [0_u8; 16];
    getrandom::fill(&mut salt).map_err(|error| io::Error::other(error.to_string()))?;
    let salt =
        SaltString::encode_b64(&salt).map_err(|error| io::Error::other(error.to_string()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| io::Error::other(format!("failed to hash password: {error}")))
}

pub(crate) fn verify_password(password: &str, stored_hash: &str) -> bool {
    PasswordHash::new(stored_hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Burns the same time as a real check so unknown usernames aren't
/// distinguishable by response time.
pub(crate) fn verify_password_against_dummy(password: &str) {
    static DUMMY_HASH: OnceLock<Option<String>> = OnceLock::new();
    if let Some(hash) = DUMMY_HASH.get_or_init(|| hash_password("musicd-dummy-password").ok()) {
        let _ = verify_password(password, hash);
    }
}

fn random_secret() -> io::Result<String> {
    random_hex(32)
}

fn random_hex(byte_count: usize) -> io::Result<String> {
    let mut bytes = vec![0_u8; byte_count];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(to_hex(&bytes))
}

pub(crate) fn hash_secret(secret: &str) -> String {
    to_hex(&Sha256::digest(secret.as_bytes()))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn redirect(writer: &mut ResponseWriter, location: &str) -> io::Result<()> {
    write_response_owned(
        writer,
        "303 See Other",
        &[("Location".to_string(), location.to_string())],
        None,
    )
}

/// Per-client-IP cap on failed sign-ins.
#[derive(Debug, Default)]
pub(crate) struct LoginThrottle {
    failures: Mutex<HashMap<Option<IpAddr>, (u32, i64)>>,
}

impl LoginThrottle {
    pub(crate) fn is_locked(&self, peer: Option<IpAddr>, now_unix: i64) -> bool {
        let failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        failures.get(&peer).is_some_and(|(count, window_start)| {
            *count >= LOGIN_FAILURE_LIMIT && now_unix - window_start < LOGIN_FAILURE_WINDOW_SECS
        })
    }

    pub(crate) fn record_failure(&self, peer: Option<IpAddr>, now_unix: i64) {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        failures.retain(|_, (_, start)| now_unix - *start < LOGIN_FAILURE_WINDOW_SECS);
        let entry = failures.entry(peer).or_insert((0, now_unix));
        entry.0 += 1;
    }

    pub(crate) fn record_success(&self, peer: Option<IpAddr>) {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        failures.remove(&peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(must_change_password: bool) -> Credentials {
        Credentials::Valid(Principal::User {
            user_id: 1,
            username: "admin".to_string(),
            must_change_password,
            session_hash: "hash".to_string(),
        })
    }

    fn token(scope: TokenScope) -> Credentials {
        Credentials::Valid(Principal::Token {
            id: "id".to_string(),
            name: "phone".to_string(),
            scope,
        })
    }

    #[test]
    fn classifies_routes() {
        assert_eq!(route_access("GET", "/health"), RouteAccess::Public);
        assert_eq!(
            route_access("GET", "/stream/track/abc"),
            RouteAccess::Public
        );
        assert_eq!(
            route_access("GET", "/artwork/album/abc"),
            RouteAccess::Public
        );
        assert_eq!(route_access("GET", "/assets/home.css"), RouteAccess::Public);
        assert_eq!(route_access("POST", "/login"), RouteAccess::Public);
        assert_eq!(route_access("GET", "/api/tracks"), RouteAccess::Standard);
        assert_eq!(route_access("POST", "/api/play"), RouteAccess::Standard);
        assert_eq!(route_access("GET", "/"), RouteAccess::Standard);
        assert_eq!(route_access("POST", "/mcp"), RouteAccess::Standard);
        assert_eq!(
            route_access("GET", "/api/recommendations"),
            RouteAccess::Standard
        );
        assert_eq!(
            route_access("DELETE", "/api/recommendations"),
            RouteAccess::Admin
        );
        assert_eq!(
            route_access("POST", "/api/recommendations/import"),
            RouteAccess::Admin
        );
        assert_eq!(route_access("POST", "/rescan"), RouteAccess::Admin);
        assert_eq!(route_access("GET", "/rescan-progress"), RouteAccess::Admin);
        assert_eq!(
            route_access("POST", "/api/tidal/auth-url"),
            RouteAccess::Admin
        );
        assert_eq!(route_access("GET", "/api/auth/tokens"), RouteAccess::Admin);
        assert_eq!(
            route_access("POST", "/api/auth/tokens/revoke"),
            RouteAccess::Admin
        );
    }

    #[test]
    fn off_mode_allows_everything() {
        for access in [
            RouteAccess::Public,
            RouteAccess::Standard,
            RouteAccess::Admin,
        ] {
            assert_eq!(
                decide(AuthMode::Off, access, &Credentials::InvalidBearer),
                Decision::Allow
            );
        }
    }

    #[test]
    fn optional_mode_only_guards_admin_routes() {
        let mode = AuthMode::Optional;
        assert_eq!(
            decide(mode, RouteAccess::Standard, &Credentials::Anonymous),
            Decision::Allow
        );
        assert_eq!(
            decide(mode, RouteAccess::Admin, &Credentials::Anonymous),
            Decision::Unauthorized
        );
        assert_eq!(
            decide(mode, RouteAccess::Admin, &user(false)),
            Decision::Allow
        );
        assert_eq!(
            decide(mode, RouteAccess::Admin, &token(TokenScope::Admin)),
            Decision::Allow
        );
        assert_eq!(
            decide(mode, RouteAccess::Admin, &token(TokenScope::Client)),
            Decision::Forbidden
        );
        assert_eq!(
            decide(mode, RouteAccess::Standard, &Credentials::InvalidBearer),
            Decision::Unauthorized
        );
    }

    #[test]
    fn required_mode_guards_standard_routes() {
        let mode = AuthMode::Required;
        assert_eq!(
            decide(mode, RouteAccess::Standard, &Credentials::Anonymous),
            Decision::Unauthorized
        );
        assert_eq!(
            decide(mode, RouteAccess::Standard, &token(TokenScope::Client)),
            Decision::Allow
        );
        assert_eq!(
            decide(mode, RouteAccess::Public, &Credentials::Anonymous),
            Decision::Allow
        );
    }

    #[test]
    fn pending_password_change_blocks_everything_but_public_routes() {
        for mode in [AuthMode::Optional, AuthMode::Required] {
            assert_eq!(
                decide(mode, RouteAccess::Standard, &user(true)),
                Decision::PasswordChangeRequired
            );
            assert_eq!(
                decide(mode, RouteAccess::Admin, &user(true)),
                Decision::PasswordChangeRequired
            );
            assert_eq!(
                decide(mode, RouteAccess::Public, &user(true)),
                Decision::Allow
            );
        }
    }

    #[test]
    fn parses_bearer_tokens_and_cookies() {
        assert_eq!(bearer_token("Bearer mdt_abc"), Some("mdt_abc"));
        assert_eq!(bearer_token("bearer   mdt_abc "), Some("mdt_abc"));
        assert_eq!(bearer_token("Basic dXNlcjpwYXNz"), None);
        assert_eq!(bearer_token("Bearer "), None);
        assert_eq!(
            cookie_value("theme=dark; musicd_session=abc123; other=1", SESSION_COOKIE),
            Some("abc123")
        );
        assert_eq!(cookie_value("xmusicd_session=abc", SESSION_COOKIE), None);
        assert_eq!(cookie_value("musicd_session=", SESSION_COOKIE), None);
    }

    #[test]
    fn rejects_off_site_redirect_targets() {
        assert!(is_safe_redirect_target("/library?facet=albums"));
        assert!(!is_safe_redirect_target("//evil.example"));
        assert!(!is_safe_redirect_target("/\\evil.example"));
        assert!(!is_safe_redirect_target("https://evil.example"));
        assert!(!is_safe_redirect_target("/ok\r\nSet-Cookie: x=1"));
    }

    #[test]
    fn hashes_and_verifies_passwords() {
        let hash = hash_password("correct horse").expect("hash");
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("correct horse", &hash));
        assert!(!verify_password("wrong horse", &hash));
        assert!(!verify_password("correct horse", "not-a-hash"));
    }

    #[test]
    fn throttles_repeated_login_failures_per_client() {
        let throttle = LoginThrottle::default();
        let peer: Option<IpAddr> = "192.168.1.9".parse().ok();
        let other: Option<IpAddr> = "192.168.1.10".parse().ok();
        for _ in 0..LOGIN_FAILURE_LIMIT {
            assert!(!throttle.is_locked(peer, 1_000));
            throttle.record_failure(peer, 1_000);
        }
        assert!(throttle.is_locked(peer, 1_000));
        assert!(!throttle.is_locked(other, 1_000));
        assert!(!throttle.is_locked(peer, 1_000 + LOGIN_FAILURE_WINDOW_SECS));
        throttle.record_success(peer);
        assert!(!throttle.is_locked(peer, 1_000));
    }

    #[test]
    fn seeds_default_admin_once_and_mints_tokens() {
        let path = std::env::temp_dir().join(format!(
            "musicd-auth-test-{}-{}",
            std::process::id(),
            now_unix_timestamp()
        ));
        let database = Database::open(&path).expect("database");
        ensure_default_admin(&database).expect("seed");
        ensure_default_admin(&database).expect("seed again");
        assert_eq!(database.count_users().expect("count"), 1);
        let admin = database
            .load_user_by_username(DEFAULT_ADMIN_USERNAME)
            .expect("load")
            .expect("admin exists");
        assert!(admin.must_change_password);
        assert!(verify_password(
            DEFAULT_ADMIN_PASSWORD,
            &admin.password_hash
        ));

        let (record, secret) =
            create_api_token(&database, "phone", TokenScope::Client).expect("token");
        assert!(secret.starts_with(API_TOKEN_PREFIX));
        let request = HttpRequest {
            authorization: Some(format!("Bearer {secret}")),
            ..HttpRequest::default()
        };
        assert_eq!(
            authenticate(&database, &request).expect("auth"),
            Credentials::Valid(Principal::Token {
                id: record.id.clone(),
                name: "phone".to_string(),
                scope: TokenScope::Client,
            })
        );
        assert!(database.revoke_api_token(&record.id, 1).expect("revoke"));
        assert_eq!(
            authenticate(&database, &request).expect("auth"),
            Credentials::InvalidBearer
        );

        let session = start_web_session(&database, admin.id).expect("session");
        let request = HttpRequest {
            cookie: Some(format!("{SESSION_COOKIE}={session}")),
            ..HttpRequest::default()
        };
        assert!(matches!(
            authenticate(&database, &request).expect("auth"),
            Credentials::Valid(Principal::User {
                must_change_password: true,
                ..
            })
        ));

        let _ = std::fs::remove_dir_all(&path);
    }
}
