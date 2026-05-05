use crate::util::html_escape;

pub(crate) fn render_detail_error_page(message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Track Error</title></head>
<body style="font-family: Georgia, serif; margin: 2rem;">
  <h1>Track Inspector</h1>
  <p>{}</p>
  <p><a href="/">Back to Library</a></p>
</body>
</html>"#,
        html_escape(message)
    )
}

