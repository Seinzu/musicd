use std::io;

use musicd_core::AuthMode;
use serde_json::json;

use crate::db::ApiTokenRecord;
use crate::http::{
    HttpRequest, ResponseWriter, api_error, request_value, respond_json, write_response_owned,
};
use crate::service::ServiceState;
use crate::util::{now_unix_timestamp, url_encode};
use crate::views::{render_change_password_page, render_login_page};

use super::{
    Credentials, DEFAULT_ADMIN_PASSWORD, MIN_PASSWORD_LENGTH, Principal, TokenScope, authenticate,
    cleared_session_cookie, cookie_value, create_api_token, hash_password, hash_secret,
    is_safe_redirect_target, session_cookie, start_web_session, verify_password,
    verify_password_against_dummy,
};

struct SignedInUser {
    user_id: i64,
    username: String,
    must_change_password: bool,
    session_hash: String,
}

fn signed_in_user(state: &ServiceState, request: &HttpRequest) -> io::Result<Option<SignedInUser>> {
    Ok(match authenticate(&state.database, request)? {
        Credentials::Valid(Principal::User {
            user_id,
            username,
            must_change_password,
            session_hash,
        }) => Some(SignedInUser {
            user_id,
            username,
            must_change_password,
            session_hash,
        }),
        _ => None,
    })
}

fn next_param(request: &HttpRequest) -> &str {
    request_value(request, "next")
        .filter(|next| is_safe_redirect_target(next))
        .unwrap_or("/")
}

fn redirect_with_headers(
    writer: &mut ResponseWriter,
    location: &str,
    extra_headers: &[(String, String)],
) -> io::Result<()> {
    let mut headers = vec![
        ("Location".to_string(), location.to_string()),
        ("Cache-Control".to_string(), "no-store".to_string()),
    ];
    headers.extend_from_slice(extra_headers);
    write_response_owned(writer, "303 See Other", &headers, None)
}

fn respond_page(writer: &mut ResponseWriter, status: &str, body: &str) -> io::Result<()> {
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

fn password_change_location(next: &str) -> String {
    if next == "/" {
        "/account/password".to_string()
    } else {
        format!("/account/password?next={}", url_encode(next))
    }
}

pub(crate) fn handle_login_form_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let next = next_param(request);
    if state.config.auth_mode == AuthMode::Off {
        return redirect_with_headers(writer, next, &[]);
    }
    if let Some(user) = signed_in_user(state, request)? {
        let location = if user.must_change_password {
            password_change_location(next)
        } else {
            next.to_string()
        };
        return redirect_with_headers(writer, &location, &[]);
    }
    let body = render_login_page(&state.config.instance_name, next, None);
    respond_page(writer, "200 OK", &body)
}

pub(crate) fn handle_login_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let next = next_param(request);
    let instance_name = &state.config.instance_name;
    let now = now_unix_timestamp();
    if state.login_throttle.is_locked(request.peer, now) {
        let body = render_login_page(
            instance_name,
            next,
            Some("Too many failed sign-in attempts. Try again in a few minutes."),
        );
        return respond_page(writer, "429 Too Many Requests", &body);
    }

    let username = request_value(request, "username").unwrap_or("").trim();
    let password = request_value(request, "password").unwrap_or("");
    let user = state.database.load_user_by_username(username)?;
    let verified = match &user {
        Some(user) => verify_password(password, &user.password_hash),
        None => {
            verify_password_against_dummy(password);
            false
        }
    };
    let Some(user) = user.filter(|_| verified) else {
        state.login_throttle.record_failure(request.peer, now);
        let body = render_login_page(instance_name, next, Some("Incorrect username or password."));
        return respond_page(writer, "401 Unauthorized", &body);
    };

    state.login_throttle.record_success(request.peer);
    let secret = start_web_session(&state.database, user.id)?;
    let location = if user.must_change_password {
        password_change_location(next)
    } else {
        next.to_string()
    };
    redirect_with_headers(
        writer,
        &location,
        &[("Set-Cookie".to_string(), session_cookie(&secret))],
    )
}

pub(crate) fn handle_logout_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    if let Some(secret) = request
        .cookie
        .as_deref()
        .and_then(|header| cookie_value(header, super::SESSION_COOKIE))
    {
        state.database.delete_web_session(&hash_secret(secret))?;
    }
    redirect_with_headers(
        writer,
        "/login",
        &[("Set-Cookie".to_string(), cleared_session_cookie())],
    )
}

pub(crate) fn handle_change_password_form_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let next = next_param(request);
    let Some(user) = signed_in_user(state, request)? else {
        return redirect_with_headers(
            writer,
            &format!(
                "/login?next={}",
                url_encode(&password_change_location(next))
            ),
            &[],
        );
    };
    let body = render_change_password_page(
        &state.config.instance_name,
        &user.username,
        user.must_change_password,
        next,
        None,
    );
    respond_page(writer, "200 OK", &body)
}

pub(crate) fn handle_change_password_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let next = next_param(request);
    let Some(user) = signed_in_user(state, request)? else {
        return redirect_with_headers(writer, "/login?next=%2Faccount%2Fpassword", &[]);
    };
    let current = request_value(request, "current_password").unwrap_or("");
    let new_password = request_value(request, "new_password").unwrap_or("");
    let confirm = request_value(request, "confirm_password").unwrap_or("");

    let stored = state
        .database
        .load_user(user.user_id)?
        .ok_or_else(|| io::Error::other("signed-in user no longer exists"))?;
    let error = if state
        .login_throttle
        .is_locked(request.peer, now_unix_timestamp())
    {
        Some("Too many failed attempts. Try again in a few minutes.")
    } else if !verify_password(current, &stored.password_hash) {
        state
            .login_throttle
            .record_failure(request.peer, now_unix_timestamp());
        Some("Current password is incorrect.")
    } else {
        validate_new_password(current, new_password, confirm)
    };
    if let Some(error) = error {
        let body = render_change_password_page(
            &state.config.instance_name,
            &user.username,
            user.must_change_password,
            next,
            Some(error),
        );
        return respond_page(writer, "400 Bad Request", &body);
    }

    let hash = hash_password(new_password)?;
    state
        .database
        .update_user_password(user.user_id, &hash, now_unix_timestamp())?;
    state
        .database
        .delete_other_web_sessions(user.user_id, &user.session_hash)?;
    let separator = if next.contains('?') { '&' } else { '?' };
    redirect_with_headers(
        writer,
        &format!("{next}{separator}message=Password+changed."),
        &[],
    )
}

fn validate_new_password(current: &str, new_password: &str, confirm: &str) -> Option<&'static str> {
    if new_password.chars().count() < MIN_PASSWORD_LENGTH {
        Some("New password is too short.")
    } else if new_password != confirm {
        Some("New passwords don't match.")
    } else if new_password == current || new_password == DEFAULT_ADMIN_PASSWORD {
        Some("Choose a password different from the current one.")
    } else {
        None
    }
}

fn api_token_json(record: &ApiTokenRecord) -> serde_json::Value {
    json!({
        "id": record.id,
        "name": record.name,
        "scope": record.scope,
        "created_unix": record.created_unix,
        "last_used_unix": record.last_used_unix,
        "revoked_unix": record.revoked_unix,
    })
}

pub(crate) fn handle_api_tokens_list_request(
    writer: &mut ResponseWriter,
    _request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let tokens = state.database.list_api_tokens()?;
    let body = json!({
        "ok": true,
        "tokens": tokens.iter().map(api_token_json).collect::<Vec<_>>(),
    });
    respond_json(writer, "200 OK", &body.to_string())
}

pub(crate) fn handle_api_tokens_create_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let name = request_value(request, "name").unwrap_or("").trim();
    if name.is_empty() || name.chars().count() > 100 {
        return api_error(writer, "400 Bad Request", "name must be 1-100 characters");
    }
    let Some(scope) = request_value(request, "scope").and_then(TokenScope::parse) else {
        return api_error(
            writer,
            "400 Bad Request",
            "scope must be 'admin' or 'client'",
        );
    };
    let (record, secret) = create_api_token(&state.database, name, scope)?;
    let mut body = api_token_json(&record);
    body["token"] = json!(secret);
    let body = json!({ "ok": true, "token": body });
    respond_json(writer, "201 Created", &body.to_string())
}

pub(crate) fn handle_api_tokens_revoke_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let Some(id) = request_value(request, "id")
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return api_error(writer, "400 Bad Request", "id is required");
    };
    if !state.database.revoke_api_token(id, now_unix_timestamp())? {
        return api_error(writer, "404 Not Found", "no active token with that id");
    }
    respond_json(writer, "200 OK", r#"{"ok":true}"#)
}

#[cfg(test)]
mod tests {
    use super::validate_new_password;

    #[test]
    fn validates_new_passwords() {
        assert!(validate_new_password("password", "short", "short").is_some());
        assert!(validate_new_password("password", "long enough", "long enougj").is_some());
        assert!(validate_new_password("old secret", "old secret", "old secret").is_some());
        assert!(validate_new_password("old secret", "password", "password").is_some());
        assert_eq!(
            validate_new_password("password", "a better one", "a better one"),
            None
        );
    }
}
