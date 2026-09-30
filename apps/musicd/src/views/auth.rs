use crate::assets;
use crate::auth::MIN_PASSWORD_LENGTH;
use crate::util::html_escape;

fn render_auth_page(instance_name: &str, title: &str, body_html: &str) -> String {
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
  <main id="main-content" class="page-shell auth-shell">
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
