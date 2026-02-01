//! Asset management and embedding.

use include_dir::{include_dir, Dir};

// Embed all assets at compile time
static ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets");

/// Get index.html
pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Kimberlite Studio</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            margin: 0;
            padding: 2rem;
            background: #fafafa;
        }
        .container {
            max-width: 1200px;
            margin: 0 auto;
        }
        h1 {
            color: #333;
            margin-bottom: 2rem;
        }
        .placeholder {
            padding: 2rem;
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }
        .info {
            color: #666;
            margin-bottom: 1rem;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>Kimberlite Studio</h1>
        <div class="placeholder">
            <div class="info">
                Studio UI is running! 🎉
            </div>
            <div class="info">
                Full query editor, results table, and time-travel UI will be implemented in Phase 3 Task #7.
            </div>
            <div class="info">
                For now, use the REPL: <code>kmb repl --tenant 1</code>
            </div>
        </div>
    </div>
</body>
</html>
"#;

/// Get a CSS file by path.
pub fn get_css(path: &str) -> Option<&'static [u8]> {
    let full_path = format!("css/{}", path);
    ASSETS.get_file(&full_path).map(|f| f.contents())
}

/// Get a font file by path.
pub fn get_font(path: &str) -> Option<&'static [u8]> {
    let full_path = format!("fonts/{}", path);
    ASSETS.get_file(&full_path).map(|f| f.contents())
}

/// Get a vendor file by path.
pub fn get_vendor(path: &str) -> Option<&'static [u8]> {
    let full_path = format!("vendor/{}", path);
    ASSETS.get_file(&full_path).map(|f| f.contents())
}
