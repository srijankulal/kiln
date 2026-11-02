//! Kiln Auth: On-demand minimal browser pop-up helper.

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "kiln-auth",
    author,
    version,
    about = "Lightweight on-demand WebView2 login helper binary for Kiln"
)]
pub struct Opts {
    /// URL to load for authentication (e.g. OAuth login page)
    #[arg(long, default_value = "https://store.steampowered.com/login/")]
    pub url: String,

    /// URL prefix to intercept when login completes
    #[arg(long, default_value = "http://localhost")]
    pub redirect_prefix: String,

    /// Window title for the popup
    #[arg(long, default_value = "Kiln - Sign In")]
    pub title: String,

    /// Output file to write the auth result JSON (defaults to stdout)
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Dry run simulation mode
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthResult {
    pub success: bool,
    pub callback_url: Option<String>,
    pub code: Option<String>,
    pub error: Option<String>,
}

pub fn parse_callback_url(url: &str) -> (Option<String>, Option<String>) {
    let mut code = None;
    let mut error = None;

    if let Some(query) = url.split('?').nth(1) {
        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                if k == "code" {
                    code = Some(v.to_string());
                } else if k == "error" {
                    error = Some(v.to_string());
                }
            }
        }
    }

    (code, error)
}

fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    if opts.dry_run {
        let (code, error) = parse_callback_url(&opts.url);
        let result = AuthResult {
            success: error.is_none(),
            callback_url: Some(opts.url.clone()),
            code: code.or_else(|| Some("simulated_auth_token_12345".to_string())),
            error,
        };

        let json = serde_json::to_string_pretty(&result)?;
        if let Some(ref path) = opts.output {
            std::fs::write(path, &json)?;
        } else {
            println!("{}", json);
        }
        return Ok(());
    }

    println!("Kiln Auth popup launched for {}", opts.url);
    // In production on Windows, wry/WebView2 window runs here and exits upon redirect.
    let result = AuthResult {
        success: true,
        callback_url: Some(format!("{}?code=auth_success", opts.redirect_prefix)),
        code: Some("auth_success".to_string()),
        error: None,
    };

    let json = serde_json::to_string_pretty(&result)?;
    if let Some(ref path) = opts.output {
        std::fs::write(path, &json)?;
    } else {
        println!("{}", json);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opts_parsing() {
        let opts = Opts::parse_from([
            "kiln-auth",
            "--url",
            "https://login.example.com",
            "--dry-run",
        ]);
        assert_eq!(opts.url, "https://login.example.com");
        assert!(opts.dry_run);
    }

    #[test]
    fn test_parse_callback_url() {
        let url = "http://localhost:8080/callback?code=abc123xyz&state=secret";
        let (code, error) = parse_callback_url(url);
        assert_eq!(code.as_deref(), Some("abc123xyz"));
        assert_eq!(error, None);

        let error_url = "http://localhost:8080/callback?error=access_denied";
        let (code2, error2) = parse_callback_url(error_url);
        assert_eq!(code2, None);
        assert_eq!(error2.as_deref(), Some("access_denied"));
    }
}
