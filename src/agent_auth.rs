use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use clap::{Args, Subcommand};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Config, expand_tilde, read_remote_auth_record, set_owner_read_write_only,
    write_remote_auth_record,
};

const DEFAULT_REFRESH_MARGIN_SECONDS: u64 = 300;

const GROK_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const GROK_ISSUER: &str = "https://auth.x.ai";
const GROK_DEFAULT_AUTH_ENDPOINT: &str = "https://auth.x.ai/oauth2/token";
const GROK_AUTH_ENV_VAR: &str = "GROK_CODE_XAI_API_KEY";

const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_ISSUER: &str = "https://auth.openai.com";
const CODEX_DEFAULT_AUTH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CODEX_REFRESH_MAX_AGE_SECONDS: u64 = 8 * 24 * 60 * 60;

#[derive(Args)]
pub(crate) struct ImportArgs {
    #[command(subcommand)]
    command: ImportCommand,
}

#[derive(Subcommand)]
enum ImportCommand {
    /// Import a Grok auth.json record.
    Grok(ImportProviderArgs),
    /// Import a Codex auth.json record.
    Codex(ImportProviderArgs),
}

#[derive(Args)]
struct ImportProviderArgs {
    /// Path to auth JSON. Pass - to read stdin.
    path: String,
}

#[derive(Args)]
pub(crate) struct RunArgs {
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
    /// Refresh before expiry by this many seconds.
    #[arg(long, default_value_t = DEFAULT_REFRESH_MARGIN_SECONDS)]
    refresh_margin_secs: u64,
    /// Arguments passed to the tool.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<OsString>,
}

#[derive(Args)]
pub(crate) struct CodexArgs {
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
    /// Refresh before expiry by this many seconds.
    #[arg(long, default_value_t = DEFAULT_REFRESH_MARGIN_SECONDS)]
    refresh_margin_secs: u64,
    /// Temporarily replace an existing Codex auth.json and restore it after exit.
    #[arg(long, short)]
    force: bool,
    /// Arguments passed to Codex.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<OsString>,
}

#[derive(Clone, Copy)]
enum Provider {
    Grok,
    Codex,
}

impl Provider {
    fn name(self) -> &'static str {
        match self {
            Provider::Grok => "grok",
            Provider::Codex => "codex",
        }
    }

    fn client_id(self) -> &'static str {
        match self {
            Provider::Grok => GROK_CLIENT_ID,
            Provider::Codex => CODEX_CLIENT_ID,
        }
    }

    fn issuer(self) -> &'static str {
        match self {
            Provider::Grok => GROK_ISSUER,
            Provider::Codex => CODEX_ISSUER,
        }
    }

    fn endpoint(self) -> String {
        match self {
            Provider::Grok => std::env::var("GROK_AUTH_ENDPOINT_URL")
                .unwrap_or_else(|_| GROK_DEFAULT_AUTH_ENDPOINT.to_string()),
            Provider::Codex => std::env::var("CODEX_AUTH_ENDPOINT_URL")
                .unwrap_or_else(|_| CODEX_DEFAULT_AUTH_ENDPOINT.to_string()),
        }
    }

    fn refresh_body_format(self) -> RefreshBodyFormat {
        match self {
            Provider::Grok => RefreshBodyFormat::Form,
            Provider::Codex => RefreshBodyFormat::Json,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefreshBodyFormat {
    Form,
    Json,
}

#[derive(Clone, Serialize, Deserialize)]
struct AuthRecord {
    client_id: String,
    issuer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id_token: Option<String>,
    access_token: String,
    refresh_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
    expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_refresh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
    token_type: Option<String>,
}

pub(crate) fn import(args: ImportArgs, cfg: &Config) -> Result<()> {
    match args.command {
        ImportCommand::Grok(args) => import_provider(Provider::Grok, &args.path, cfg),
        ImportCommand::Codex(args) => import_provider(Provider::Codex, &args.path, cfg),
    }
}

pub(crate) fn run_grok(args: RunArgs, cfg: &Config) -> Result<()> {
    let auth = load_auth(
        cfg,
        Provider::Grok,
        args.allow_ssh_keychain,
        args.refresh_margin_secs,
    )?
    .auth;
    let mut env = BTreeMap::new();
    env.insert(GROK_AUTH_ENV_VAR.to_string(), auth.access_token);
    run_tool("grok", args.command, &env)
}

pub(crate) fn run_codex(args: CodexArgs, cfg: &Config) -> Result<()> {
    let auth = load_auth(
        cfg,
        Provider::Codex,
        args.allow_ssh_keychain,
        args.refresh_margin_secs,
    )?
    .auth;
    let prepared = prepare_codex_home(&auth, args.force)?;
    let mut env = BTreeMap::new();
    env.insert(
        "CODEX_HOME".to_string(),
        prepared.codex_home.to_string_lossy().into_owned(),
    );
    let status = tool_status("codex", args.command, &env);
    let cleanup = prepared.cleanup();
    let status = status?;
    cleanup?;
    if status.success() {
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn import_provider(provider: Provider, path: &str, cfg: &Config) -> Result<()> {
    let raw = read_auth_input(path)?;
    let auth = match provider {
        Provider::Grok => import_grok_auth(&raw)?,
        Provider::Codex => import_codex_auth(&raw)?,
    };
    write_auth_record(cfg, provider, &auth)?;
    println!("imported {} auth", provider.name());
    Ok(())
}

fn read_auth_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut raw = String::new();
        std::io::stdin()
            .read_to_string(&mut raw)
            .context("read auth JSON from stdin")?;
        return Ok(raw);
    }
    fs::read_to_string(expand_tilde(path)).with_context(|| format!("read auth JSON from {path}"))
}

fn import_grok_auth(raw: &str) -> Result<AuthRecord> {
    let content: Value = serde_json::from_str(raw).context("parse Grok auth JSON")?;
    let key = format!("{GROK_ISSUER}::{GROK_CLIENT_ID}");
    let entry = content
        .get(&key)
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("No Grok OIDC auth entry found for {key}."))?;
    let access_token = required_string(entry.get("key"), "Grok auth entry is missing key")?;
    let refresh_token = required_string(
        entry.get("refresh_token"),
        "Grok auth entry is missing refresh_token",
    )?;
    let expires_at = required_string(
        entry.get("expires_at"),
        "Grok auth entry is missing expires_at",
    )?;
    Ok(AuthRecord {
        client_id: GROK_CLIENT_ID.to_string(),
        issuer: GROK_ISSUER.to_string(),
        id_token: None,
        access_token,
        refresh_token,
        account_id: None,
        expires_at,
        last_refresh: None,
        scope: None,
        token_type: Some("Bearer".to_string()),
        updated_at: Some(now_iso()),
    })
}

fn import_codex_auth(raw: &str) -> Result<AuthRecord> {
    let content: Value = serde_json::from_str(raw).context("parse Codex auth JSON")?;
    let tokens = content
        .get("tokens")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("Codex auth file is missing tokens."))?;
    let id_token = required_string(
        tokens.get("id_token"),
        "Codex auth file is missing id_token",
    )?;
    let access_token = required_string(
        tokens.get("access_token"),
        "Codex auth file is missing access_token",
    )?;
    let refresh_token = required_string(
        tokens.get("refresh_token"),
        "Codex auth file is missing refresh_token",
    )?;
    let account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let expires_at = jwt_expiry_iso(&access_token).context("derive Codex access token expiry")?;
    let last_refresh = content
        .get("last_refresh")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(now_iso);
    Ok(AuthRecord {
        client_id: CODEX_CLIENT_ID.to_string(),
        issuer: CODEX_ISSUER.to_string(),
        id_token: Some(id_token),
        access_token,
        refresh_token,
        account_id,
        expires_at,
        last_refresh: Some(last_refresh),
        scope: None,
        token_type: Some("Bearer".to_string()),
        updated_at: Some(now_iso()),
    })
}

fn required_string(value: Option<&Value>, message: &str) -> Result<String> {
    value
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!(message.to_string()))
}

struct LoadedAuth {
    auth: AuthRecord,
}

fn load_auth(
    cfg: &Config,
    provider: Provider,
    allow_ssh_keychain: bool,
    refresh_margin_secs: u64,
) -> Result<LoadedAuth> {
    let raw = read_auth_record_raw(cfg, provider, allow_ssh_keychain)?;
    let auth = parse_auth_record(&raw, provider)?;
    if is_fresh(provider, &auth, refresh_margin_secs) {
        return Ok(LoadedAuth { auth });
    }

    let http = Client::new();
    match refresh_auth(&http, provider, &auth) {
        Ok(refreshed) => {
            write_auth_record(cfg, provider, &refreshed)?;
            Ok(LoadedAuth { auth: refreshed })
        }
        Err(err) => {
            let reread = read_auth_record_raw(cfg, provider, allow_ssh_keychain)?;
            if reread != raw {
                let newer = parse_auth_record(&reread, provider)?;
                if is_fresh(provider, &newer, refresh_margin_secs) {
                    return Ok(LoadedAuth { auth: newer });
                }
            }
            Err(err)
        }
    }
}

fn parse_auth_record(raw: &str, provider: Provider) -> Result<AuthRecord> {
    let auth: AuthRecord = serde_json::from_str(raw)
        .with_context(|| format!("parse {} auth JSON", provider.name()))?;
    if auth.client_id != provider.client_id()
        || auth.issuer != provider.issuer()
        || auth.access_token.trim().is_empty()
        || auth.refresh_token.trim().is_empty()
        || auth.expires_at.trim().is_empty()
    {
        bail!(
            "stored auth JSON for {} is missing required fields or has the wrong client",
            provider.name()
        );
    }
    if matches!(provider, Provider::Codex) && auth.id_token.as_deref().unwrap_or("").is_empty() {
        bail!("stored Codex auth is missing id_token; re-run `rage import codex <auth-file>`");
    }
    Ok(auth)
}

fn refresh_auth(http: &Client, provider: Provider, auth: &AuthRecord) -> Result<AuthRecord> {
    let response = match provider.refresh_body_format() {
        RefreshBodyFormat::Form => http
            .post(provider.endpoint())
            .header("accept", "application/json")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("user-agent", "rage")
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", auth.refresh_token.as_str()),
                ("client_id", provider.client_id()),
            ])
            .send(),
        RefreshBodyFormat::Json => http
            .post(provider.endpoint())
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("user-agent", "rage")
            .json(&json!({
                "client_id": provider.client_id(),
                "grant_type": "refresh_token",
                "refresh_token": auth.refresh_token,
            }))
            .send(),
    }
    .with_context(|| format!("refresh {} auth", provider.name()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .unwrap_or_else(|_| "<unreadable body>".to_string());
        let safe_body = safe_error_body(&body);
        if body.contains("invalid_grant") {
            bail!(
                "{} auth refresh failed: HTTP {} {}\nThe stored refresh token is no longer valid. Re-run `rage import {} <auth-file>` from a logged-in machine.",
                provider.name(),
                status.as_u16(),
                safe_body,
                provider.name()
            );
        }
        bail!(
            "{} auth refresh failed: HTTP {} {}",
            provider.name(),
            status.as_u16(),
            safe_body
        );
    }

    let data: RefreshResponse = response
        .json()
        .with_context(|| format!("parse {} refresh response", provider.name()))?;
    if provider.refresh_body_format() == RefreshBodyFormat::Form
        && (data.access_token.is_none()
            || data.refresh_token.is_none()
            || data.expires_in.is_none())
    {
        bail!(
            "{} refresh response is missing access_token, refresh_token, or expires_in",
            provider.name()
        );
    }

    let access_token = data
        .access_token
        .unwrap_or_else(|| auth.access_token.clone());
    let refresh_token = data
        .refresh_token
        .unwrap_or_else(|| auth.refresh_token.clone());
    let id_token = data.id_token.or_else(|| auth.id_token.clone());
    let expires_at = if let Some(expires_in) = data.expires_in {
        unix_to_iso(now_unix() + expires_in)
    } else {
        jwt_expiry_iso(&access_token)?
    };

    Ok(AuthRecord {
        client_id: provider.client_id().to_string(),
        issuer: provider.issuer().to_string(),
        id_token,
        access_token,
        refresh_token,
        account_id: auth.account_id.clone(),
        expires_at,
        last_refresh: if provider.refresh_body_format() == RefreshBodyFormat::Json {
            Some(now_iso())
        } else {
            auth.last_refresh.clone()
        },
        scope: data.scope,
        token_type: data.token_type,
        updated_at: Some(now_iso()),
    })
}

fn read_auth_record_raw(
    cfg: &Config,
    provider: Provider,
    _allow_ssh_keychain: bool,
) -> Result<String> {
    read_remote_auth_record(cfg, provider.name())
}

fn write_auth_record(cfg: &Config, provider: Provider, auth: &AuthRecord) -> Result<()> {
    let raw = serde_json::to_string_pretty(auth)?;
    write_remote_auth_record(cfg, provider.name(), &raw)
}

fn is_fresh(provider: Provider, auth: &AuthRecord, margin_seconds: u64) -> bool {
    let Some(expires_at) = parse_iso_unix(&auth.expires_at) else {
        return false;
    };
    let now = now_unix();
    if now.saturating_add(margin_seconds) >= expires_at {
        return false;
    }
    if matches!(provider, Provider::Codex) {
        let Some(last_refresh) = auth.last_refresh.as_deref().and_then(parse_iso_unix) else {
            return false;
        };
        return now < last_refresh.saturating_add(CODEX_REFRESH_MAX_AGE_SECONDS);
    }
    true
}

fn run_tool(program: &str, args: Vec<OsString>, env: &BTreeMap<String, String>) -> Result<()> {
    let status = tool_status(program, args, env)?;
    if status.success() {
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn tool_status(
    program: &str,
    args: Vec<OsString>,
    env: &BTreeMap<String, String>,
) -> Result<std::process::ExitStatus> {
    let args = strip_leading_dashdash(args);
    let mut child = Command::new(program)
        .args(args)
        .envs(env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("start {program}"))?;
    child.wait().map_err(Into::into)
}

fn strip_leading_dashdash(args: Vec<OsString>) -> Vec<OsString> {
    let mut args = args;
    if args
        .first()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value == "--")
    {
        args.remove(0);
    }
    args
}

struct PreparedCodexHome {
    codex_home: PathBuf,
    auth_path: PathBuf,
    backup_path: Option<PathBuf>,
    had_existing_auth: bool,
    force: bool,
}

impl PreparedCodexHome {
    fn cleanup(&self) -> Result<()> {
        if self.force && self.had_existing_auth {
            if let Some(backup_path) = &self.backup_path {
                fs::copy(backup_path, &self.auth_path).with_context(|| {
                    format!("restore Codex auth.json from {}", backup_path.display())
                })?;
                fs::remove_file(backup_path)
                    .with_context(|| format!("remove {}", backup_path.display()))?;
            }
        } else if self.force || !self.had_existing_auth {
            match fs::remove_file(&self.auth_path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("remove {}", self.auth_path.display()));
                }
            }
        }
        Ok(())
    }
}

fn prepare_codex_home(auth: &AuthRecord, force: bool) -> Result<PreparedCodexHome> {
    let codex_home = codex_home();
    let auth_path = codex_home.join("auth.json");
    let backup_path = codex_home.join(format!(
        ".rage-auth.json.backup-{}-{}",
        std::process::id(),
        now_unix()
    ));
    fs::create_dir_all(&codex_home).with_context(|| format!("create {}", codex_home.display()))?;
    let had_existing_auth = auth_path.exists();
    if force && had_existing_auth {
        fs::copy(&auth_path, &backup_path).with_context(|| {
            format!(
                "backup existing Codex auth.json from {}",
                auth_path.display()
            )
        })?;
        set_owner_read_write_only(&backup_path)?;
    }
    fs::write(&auth_path, codex_auth_json(auth)?)
        .with_context(|| format!("write {}", auth_path.display()))?;
    set_owner_read_write_only(&auth_path)?;
    Ok(PreparedCodexHome {
        codex_home,
        auth_path,
        backup_path: (force && had_existing_auth).then_some(backup_path),
        had_existing_auth,
        force,
    })
}

fn codex_home() -> PathBuf {
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        return PathBuf::from(home);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".codex")
}

fn codex_auth_json(auth: &AuthRecord) -> Result<String> {
    let id_token = auth
        .id_token
        .as_deref()
        .ok_or_else(|| anyhow!("stored Codex auth is missing id_token"))?;
    let mut tokens = serde_json::Map::new();
    tokens.insert("id_token".to_string(), json!(id_token));
    tokens.insert("access_token".to_string(), json!(auth.access_token));
    tokens.insert("refresh_token".to_string(), json!(auth.refresh_token));
    if let Some(account_id) = &auth.account_id {
        tokens.insert("account_id".to_string(), json!(account_id));
    }
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": Value::Null,
            "tokens": tokens,
            "last_refresh": auth.last_refresh.clone().unwrap_or_else(now_iso),
        }))?
    ))
}

fn jwt_expiry_iso(jwt: &str) -> Result<String> {
    let payload = jwt
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow!("JWT is missing a payload"))?;
    let parsed: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(payload.as_bytes())
            .context("decode JWT payload")?,
    )
    .context("parse JWT payload")?;
    let exp = parsed
        .get("exp")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("JWT payload is missing numeric exp"))?;
    Ok(unix_to_iso(exp))
}

fn safe_error_body(body: &str) -> String {
    let mut out = if let Ok(parsed) = serde_json::from_str::<Value>(body) {
        let mut safe = serde_json::Map::new();
        for key in ["error", "error_description", "message", "detail"] {
            if let Some(value) = parsed.get(key).and_then(Value::as_str) {
                safe.insert(key.to_string(), json!(redact_secret_like(value)));
            }
        }
        if safe.is_empty() {
            redact_secret_like(body)
        } else {
            Value::Object(safe).to_string()
        }
    } else {
        redact_secret_like(body)
    };
    out.truncate(500);
    out
}

fn redact_secret_like(value: &str) -> String {
    value
        .split_whitespace()
        .map(|part| {
            if part.starts_with("eyJ") && part.matches('.').count() >= 2 {
                "[redacted-jwt]"
            } else if part.starts_with("refresh") || part.starts_with("access") {
                "[redacted]"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

fn now_iso() -> String {
    unix_to_iso(now_unix())
}

fn parse_iso_unix(value: &str) -> Option<u64> {
    let (date, rest) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    let time = rest.trim_end_matches('Z');
    let time = time.split('.').next().unwrap_or(time);
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second = time_parts.next()?.parse::<u32>().ok()?;
    let days = days_from_civil(year, month, day)?;
    Some(days as u64 * 86_400 + hour as u64 * 3_600 + minute as u64 * 60 + second as u64)
}

fn unix_to_iso(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64;
    let seconds = timestamp % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3_600;
    let minute = (seconds % 3_600) / 60;
    let second = seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.000Z")
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = year - (month <= 2) as i32;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month as i32 + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) as i64)
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    ((y + (m <= 2) as i64) as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_iso_round_trips_current_format() {
        let timestamp = 1_797_420_096;
        let iso = unix_to_iso(timestamp);
        assert_eq!(parse_iso_unix(&iso), Some(timestamp));
    }

    #[test]
    fn redacts_token_like_refresh_errors() {
        let safe = safe_error_body(
            r#"{"error":"invalid_grant","error_description":"refresh-invalid should not print eyJabc.def.ghi"}"#,
        );
        assert!(!safe.contains("refresh-invalid"));
        assert!(!safe.contains("eyJabc.def.ghi"));
    }
}
