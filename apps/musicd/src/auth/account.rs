//! The `/account` page (pairing approval, token management) and the public
//! pairing API that clients poll.

use std::io;

use serde_json::json;

use crate::http::{
    HttpRequest, ResponseWriter, api_error, request_value, respond_json, write_response_owned,
};
use crate::service::ServiceState;
use crate::util::{now_unix_timestamp, url_encode};
use crate::views::{AccountPage, PendingPairingView, render_account_page};

use super::pairing::{
    CODE_LENGTH, PAIRING_POLL_INTERVAL_SECS, PAIRING_TTL_SECS, PairingStatus, PollOutcome,
    StartError,
};
use super::{
    Credentials, Principal, TokenScope, authenticate, create_api_token, hash_secret, random_secret,
};

const MAX_NAME_CHARS: usize = 100;

#[derive(Default)]
struct PageExtras<'a> {
    message: Option<&'a str>,
    error: Option<&'a str>,
    pending_pairing: Option<PendingPairingView>,
    new_token: Option<(&'a str, &'a str)>,
}

fn respond_account_page(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    status: &str,
    extras: PageExtras<'_>,
) -> io::Result<()> {
    let username = match authenticate(&state.database, request)? {
        Credentials::Valid(Principal::User { username, .. }) => Some(username),
        _ => None,
    };
    let tokens = state.database.list_api_tokens()?;
    let body = render_account_page(&AccountPage {
        instance_name: &state.config.instance_name,
        username: username.as_deref(),
        tokens: &tokens,
        now_unix: now_unix_timestamp(),
        message: extras.message,
        error: extras.error,
        pending_pairing: extras.pending_pairing,
        new_token: extras.new_token,
    });
    write_response_owned(
        writer,
        status,
        &[
            (
                "Content-Type".to_string(),
                "text/html; charset=utf-8".to_string(),
            ),
            ("Content-Length".to_string(), body.len().to_string()),
            ("Cache-Control".to_string(), "no-store".to_string()),
        ],
        Some(body.as_bytes()),
    )
}

fn redirect_to_account(writer: &mut ResponseWriter, message: &str) -> io::Result<()> {
    write_response_owned(
        writer,
        "303 See Other",
        &[(
            "Location".to_string(),
            format!("/account?message={}", url_encode(message)),
        )],
        None,
    )
}

fn valid_name(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= MAX_NAME_CHARS)
}

pub(crate) fn handle_account_page_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let extras = PageExtras {
        message: request
            .query
            .get("message")
            .map(String::as_str)
            .filter(|m| !m.is_empty()),
        ..PageExtras::default()
    };
    respond_account_page(writer, request, state, "200 OK", extras)
}

pub(crate) fn handle_account_token_create_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let Some(name) = valid_name(request_value(request, "name")) else {
        let extras = PageExtras {
            error: Some("Token names must be 1-100 characters."),
            ..PageExtras::default()
        };
        return respond_account_page(writer, request, state, "400 Bad Request", extras);
    };
    let Some(scope) = request_value(request, "scope").and_then(TokenScope::parse) else {
        let extras = PageExtras {
            error: Some("Choose the client or admin scope."),
            ..PageExtras::default()
        };
        return respond_account_page(writer, request, state, "400 Bad Request", extras);
    };
    let (record, secret) = create_api_token(&state.database, name, scope)?;
    let extras = PageExtras {
        new_token: Some((&record.name, &secret)),
        ..PageExtras::default()
    };
    respond_account_page(writer, request, state, "201 Created", extras)
}

pub(crate) fn handle_account_token_revoke_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let id = request_value(request, "id").unwrap_or("").trim();
    if !state.database.revoke_api_token(id, now_unix_timestamp())? {
        let extras = PageExtras {
            error: Some("That token doesn't exist or was already revoked."),
            ..PageExtras::default()
        };
        return respond_account_page(writer, request, state, "404 Not Found", extras);
    }
    redirect_to_account(writer, "Token revoked.")
}

fn pending_pairing_not_found(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let extras = PageExtras {
        error: Some("No pending pairing request has that code. Codes expire after 10 minutes."),
        ..PageExtras::default()
    };
    respond_account_page(writer, request, state, "404 Not Found", extras)
}

pub(crate) fn handle_account_pairing_lookup_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let now = now_unix_timestamp();
    let code = request_value(request, "code").unwrap_or("");
    let Some((_, pending)) = state.pairings.find_pending_by_code(code, now) else {
        return pending_pairing_not_found(writer, request, state);
    };
    let extras = PageExtras {
        pending_pairing: Some(PendingPairingView {
            code: pending.code,
            device_name: pending.device_name,
            peer: pending.peer.map(|peer| peer.to_string()),
            age_secs: now - pending.created_unix,
        }),
        ..PageExtras::default()
    };
    respond_account_page(writer, request, state, "200 OK", extras)
}

pub(crate) fn handle_account_pairing_approve_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let now = now_unix_timestamp();
    let code = request_value(request, "code").unwrap_or("");
    let Some((id_hash, pending)) = state.pairings.find_pending_by_code(code, now) else {
        return pending_pairing_not_found(writer, request, state);
    };
    let (record, secret) =
        create_api_token(&state.database, &pending.device_name, TokenScope::Client)?;
    if !state
        .pairings
        .resolve(&id_hash, PairingStatus::Approved { token: secret }, now)
    {
        // Expired or answered between lookup and approval: don't leave an
        // orphaned token behind.
        state.database.revoke_api_token(&record.id, now)?;
        return pending_pairing_not_found(writer, request, state);
    }
    redirect_to_account(
        writer,
        &format!(
            "Paired {}. It will finish setting up on its own.",
            pending.device_name
        ),
    )
}

pub(crate) fn handle_account_pairing_deny_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let now = now_unix_timestamp();
    let code = request_value(request, "code").unwrap_or("");
    let Some((id_hash, pending)) = state.pairings.find_pending_by_code(code, now) else {
        return pending_pairing_not_found(writer, request, state);
    };
    state.pairings.resolve(&id_hash, PairingStatus::Denied, now);
    redirect_to_account(
        writer,
        &format!("Denied pairing for {}.", pending.device_name),
    )
}

pub(crate) fn handle_api_pair_start_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let Some(name) = valid_name(request_value(request, "name")) else {
        return api_error(writer, "400 Bad Request", "name must be 1-100 characters");
    };
    let pairing_id = random_secret()?;
    let started = state.pairings.start(
        hash_secret(&pairing_id),
        name,
        request.peer,
        now_unix_timestamp(),
        || {
            let mut bytes = [0_u8; CODE_LENGTH];
            getrandom::fill(&mut bytes).ok().map(|()| bytes)
        },
    );
    match started {
        Ok(code) => {
            let body = json!({
                "ok": true,
                "pairing_id": pairing_id,
                "code": code,
                "expires_in": PAIRING_TTL_SECS,
                "poll_interval": PAIRING_POLL_INTERVAL_SECS,
            });
            respond_json(writer, "201 Created", &body.to_string())
        }
        Err(StartError::TooManyPending) => api_error(
            writer,
            "429 Too Many Requests",
            "too many pending pairing requests; try again later",
        ),
        Err(StartError::Random) => Err(io::Error::other("random source failed")),
    }
}

pub(crate) fn handle_api_pair_poll_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let Some(pairing_id) = request_value(request, "pairing_id").filter(|id| !id.is_empty()) else {
        return api_error(writer, "400 Bad Request", "pairing_id is required");
    };
    let body = match state
        .pairings
        .poll(&hash_secret(pairing_id), now_unix_timestamp())
    {
        PollOutcome::Pending => json!({ "ok": true, "status": "pending" }),
        PollOutcome::Approved { token } => {
            json!({ "ok": true, "status": "approved", "token": token })
        }
        PollOutcome::Denied => json!({ "ok": true, "status": "denied" }),
        PollOutcome::Unknown => {
            return api_error(
                writer,
                "404 Not Found",
                "unknown or expired pairing request",
            );
        }
    };
    respond_json(writer, "200 OK", &body.to_string())
}
