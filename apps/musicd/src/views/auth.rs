use std::fmt::Write;

use crate::assets;
use crate::auth::MIN_PASSWORD_LENGTH;
use crate::db::ApiTokenRecord;
use crate::util::html_escape;

fn render_auth_page(instance_name: &str, title: &str, body_html: &str) -> String {
    render_auth_page_with_class(instance_name, title, "page-shell auth-shell", body_html)
}

fn render_auth_page_with_class(
    instance_name: &str,
    title: &str,
    main_class: &str,
    body_html: &str,
) -> String {
    let home_css_version = assets::asset_version(assets::HOME_CSS);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title} · {instance}</title>
  <link rel="stylesheet" href="/assets/home.css?v={home_css_version}">
</head>
<body>
  <main id="main-content" class="{main_class}">
    {body_html}
  </main>
</body>
</html>"#,
        title = html_escape(title),
        instance = html_escape(instance_name),
    )
}

fn render_error_banner(error: Option<&str>) -> String {
    error
        .map(|error| format!("<p class=\"banner error\">{}</p>", html_escape(error)))
        .unwrap_or_default()
}

pub(crate) fn render_login_page(instance_name: &str, next: &str, error: Option<&str>) -> String {
    let body = format!(
        r#"{banner}
<section class="card">
  <div class="card-header">
    <h1>Sign in</h1>
    <p class="meta">{instance}</p>
  </div>
  <form class="auth-form" action="/login" method="post">
    <input type="hidden" name="next" value="{next}">
    <label>Username
      <input type="text" name="username" autocomplete="username" autocapitalize="none" required autofocus>
    </label>
    <label>Password
      <input type="password" name="password" autocomplete="current-password" required>
    </label>
    <button type="submit">Sign in</button>
  </form>
</section>"#,
        banner = render_error_banner(error),
        instance = html_escape(instance_name),
        next = html_escape(next),
    );
    render_auth_page(instance_name, "Sign in", &body)
}

pub(crate) fn render_change_password_page(
    instance_name: &str,
    username: &str,
    must_change: bool,
    next: &str,
    error: Option<&str>,
) -> String {
    let intro = if must_change {
        "You're signed in with the default password. Choose a new one to continue."
    } else {
        "Changing your password signs out your other browsers."
    };
    let body = format!(
        r#"{banner}
<section class="card">
  <div class="card-header">
    <h1>Change password</h1>
    <p class="meta">Signed in as {username}. {intro}</p>
  </div>
  <form class="auth-form" action="/account/password" method="post">
    <input type="hidden" name="next" value="{next}">
    <label>Current password
      <input type="password" name="current_password" autocomplete="current-password" required>
    </label>
    <label>New password (at least {min} characters)
      <input type="password" name="new_password" autocomplete="new-password" minlength="{min}" required>
    </label>
    <label>Confirm new password
      <input type="password" name="confirm_password" autocomplete="new-password" minlength="{min}" required>
    </label>
    <button type="submit">Change password</button>
  </form>
  <form class="account-form" action="/logout" method="post">
    <button type="submit" class="secondary">Sign out</button>
  </form>
</section>"#,
        banner = render_error_banner(error),
        username = html_escape(username),
        intro = html_escape(intro),
        next = html_escape(next),
        min = MIN_PASSWORD_LENGTH,
    );
    render_auth_page(instance_name, "Change password", &body)
}

pub(crate) struct PendingPairingView {
    pub(crate) code: String,
    pub(crate) device_name: String,
    pub(crate) peer: Option<String>,
    pub(crate) age_secs: i64,
}

pub(crate) struct AccountPage<'a> {
    pub(crate) instance_name: &'a str,
    pub(crate) username: Option<&'a str>,
    pub(crate) tokens: &'a [ApiTokenRecord],
    pub(crate) now_unix: i64,
    pub(crate) message: Option<&'a str>,
    pub(crate) error: Option<&'a str>,
    pub(crate) pending_pairing: Option<PendingPairingView>,
    /// A token minted by this request: (name, secret). Shown once.
    pub(crate) new_token: Option<(&'a str, &'a str)>,
}

/// "just now", "5 min ago", "3 h ago", "12 days ago".
pub(crate) fn describe_age(seconds: i64) -> String {
    match seconds.max(0) {
        0..60 => "just now".to_string(),
        s @ 60..3_600 => format!("{} min ago", s / 60),
        s @ 3_600..86_400 => format!("{} h ago", s / 3_600),
        s => format!("{} days ago", s / 86_400),
    }
}

pub(crate) fn render_account_page(page: &AccountPage<'_>) -> String {
    let mut banners = String::new();
    if let Some(message) = page.message {
        let _ = write!(
            banners,
            "<p class=\"banner success\">{}</p>",
            html_escape(message)
        );
    }
    banners.push_str(&render_error_banner(page.error));

    let new_token_html = page
        .new_token
        .map(|(name, secret)| {
            format!(
                r#"<section class="card">
  <div class="card-header">
    <h2>New token: {name}</h2>
    <p class="meta">Copy it now. It won't be shown again.</p>
  </div>
  <p><code class="token-secret">{secret}</code></p>
</section>"#,
                name = html_escape(name),
                secret = html_escape(secret),
            )
        })
        .unwrap_or_default();

    let pairing_html = match &page.pending_pairing {
        Some(pending) => format!(
            r#"<section class="card">
  <div class="card-header">
    <h2>Pair {device}?</h2>
    <p class="meta">Code {code}, requested {age}{peer}. Approving gives this device a client token.</p>
  </div>
  <div class="control-row">
    <form class="inline-form" action="/account/pairing/approve" method="post">
      <input type="hidden" name="code" value="{code}">
      <button type="submit">Approve</button>
    </form>
    <form class="inline-form" action="/account/pairing/deny" method="post">
      <input type="hidden" name="code" value="{code}">
      <button type="submit" class="secondary danger">Deny</button>
    </form>
  </div>
</section>"#,
            device = html_escape(&pending.device_name),
            code = html_escape(&pending.code),
            age = describe_age(pending.age_secs),
            peer = pending
                .peer
                .as_deref()
                .map(|peer| format!(" from {}", html_escape(peer)))
                .unwrap_or_default(),
        ),
        None => r#"<section class="card">
  <div class="card-header">
    <h2>Pair a device</h2>
    <p class="meta">Start pairing in the Android app or with <code>musicdctl pair</code>, then enter the code it shows.</p>
  </div>
  <form class="control-row" action="/account/pairing" method="post">
    <label for="pairing_code" class="visually-hidden">Pairing code</label>
    <input id="pairing_code" type="text" name="code" placeholder="ABCD-EFGH" autocomplete="off" autocapitalize="characters" required>
    <button type="submit">Find device</button>
  </form>
</section>"#
            .to_string(),
    };

    let token_rows = page
        .tokens
        .iter()
        .map(|token| {
            let last_used = token
                .last_used_unix
                .map(|unix| describe_age(page.now_unix - unix))
                .unwrap_or_else(|| "never".to_string());
            let action = match token.revoked_unix {
                Some(revoked) => format!("Revoked {}", describe_age(page.now_unix - revoked)),
                None => format!(
                    "<form class=\"inline-form\" action=\"/account/tokens/revoke\" method=\"post\">\
                     <input type=\"hidden\" name=\"id\" value=\"{}\">\
                     <button type=\"submit\" class=\"secondary danger\">Revoke</button></form>",
                    html_escape(&token.id)
                ),
            };
            format!(
                "<tr><td data-label=\"Name\">{}</td><td data-label=\"Scope\">{}</td>\
                 <td data-label=\"Created\">{}</td><td data-label=\"Last used\">{}</td>\
                 <td data-label=\"Status\">{}</td></tr>",
                html_escape(&token.name),
                html_escape(&token.scope),
                describe_age(page.now_unix - token.created_unix),
                last_used,
                action,
            )
        })
        .collect::<String>();
    let tokens_table = if page.tokens.is_empty() {
        "<p class=\"meta\">No tokens yet.</p>".to_string()
    } else {
        format!(
            "<table class=\"token-table\"><thead><tr><th>Name</th><th>Scope</th><th>Created</th>\
             <th>Last used</th><th>Status</th></tr></thead><tbody>{token_rows}</tbody></table>"
        )
    };

    let signed_in = page
        .username
        .map(|username| {
            format!(
                r#"<p class="meta">Signed in as {}. <a class="text-link" href="/account/password?next=%2Faccount">Change password</a></p>"#,
                html_escape(username)
            )
        })
        .unwrap_or_default();

    let body = format!(
        r#"{banners}
<section class="card">
  <div class="card-header">
    <h1>Account</h1>
    {signed_in}
  </div>
  <div class="control-row">
    <a class="button-link secondary" href="/">Back to {instance}</a>
    <form class="inline-form" action="/logout" method="post"><button type="submit" class="secondary">Sign out</button></form>
  </div>
</section>
{new_token_html}
{pairing_html}
<section class="card">
  <div class="card-header">
    <h2>API tokens</h2>
    <p class="meta">Client tokens can do everything the apps need. Admin tokens can also rescan, link Tidal, import recommendations and manage tokens.</p>
  </div>
  {tokens_table}
  <form class="control-row" action="/account/tokens" method="post">
    <label for="token_name" class="visually-hidden">Token name</label>
    <input id="token_name" type="text" name="name" placeholder="Token name, e.g. recommender" maxlength="100" required>
    <label for="token_scope" class="visually-hidden">Scope</label>
    <select id="token_scope" name="scope">
      <option value="client">client</option>
      <option value="admin">admin</option>
    </select>
    <button type="submit">Create token</button>
  </form>
</section>"#,
        instance = html_escape(page.instance_name),
    );
    render_auth_page_with_class(
        page.instance_name,
        "Account",
        "page-shell account-shell",
        &body,
    )
}
