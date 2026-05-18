use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
    thread,
    time::{Duration, Instant},
};

mod agent_auth;
mod tui;

use age::secrecy::ExposeSecret;
use anyhow::{Context, Result, anyhow, bail};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

const DEFAULT_SECRET_PREFIX: &str = "rage";
const CONFIG_FILE_NAME: &str = "config.toml";
const DEFAULT_GCP_ENDPOINT: &str = "https://secretmanager.googleapis.com";
pub(crate) const AGENT_AUTH_BUNDLE: &str = "agents";
const AGENT_AUTH_SECRET_PATH: &str = "/agents";
pub(crate) const AGENT_AUTH_DISPLAY_VALUE: &str = "<managed agent auth; value hidden>";

#[derive(Parser)]
#[command(name = "rage")]
#[command(version)]
#[command(about = "Fast local shell secrets backed by GCP Secret Manager and age-encrypted cache")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create or update ~/.config/rage/config.toml.
    Init(InitArgs),
    /// Show GCP authentication status.
    Auth(AuthArgs),
    /// Import agent auth records into encrypted rage storage.
    Import(agent_auth::ImportArgs),
    /// Print the active configuration.
    Config,
    /// List remote rage bundles in GCP Secret Manager.
    List(ListArgs),
    /// Set a KEY=VALUE in a remote bundle and update the encrypted local cache.
    Set(SetArgs),
    /// Remove a key from a remote bundle and update the encrypted local cache.
    Unset(KeyArgs),
    /// Delete a remote bundle and local cache file.
    DeleteBundle(DeleteBundleArgs),
    /// Print one cached value or a cached bundle.
    Get(GetArgs),
    /// Fetch remote bundles into the encrypted local cache.
    Sync(SyncArgs),
    /// Print cached bundles as shell exports, dotenv, or JSON.
    Load(LoadArgs),
    /// Run a command with cached bundles injected into its environment.
    Exec(ExecArgs),
    /// Start a login shell with cached bundles injected into its environment.
    Shell(ShellArgs),
    /// Open an SSH session or run a remote command with selected cached bundles.
    Ssh(SshArgs),
    /// Run Grok with a refreshed auth token.
    Grok(agent_auth::RunArgs),
    /// Run Codex with refreshed ChatGPT auth.
    Codex(agent_auth::CodexArgs),
    /// Open the interactive terminal UI for browsing and editing bundles.
    Tui(TuiArgs),
}

#[derive(Args)]
struct TuiArgs {
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
}

#[derive(Args)]
struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Show whether rage can see a GCP access token.
    Status,
}

#[derive(Args)]
struct InitArgs {
    /// GCP project ID. If omitted, rage reads RAGE_GCP_PROJECT or GOOGLE_CLOUD_PROJECT.
    #[arg(long)]
    gcp_project: Option<String>,
    /// Legacy alias accepted while migrating back from the Infisical-backed config.
    #[arg(long, hide = true)]
    infisical_project_id: Option<String>,
    /// GCP Secret Manager API endpoint.
    #[arg(long, default_value = DEFAULT_GCP_ENDPOINT)]
    gcp_endpoint: String,
    /// age recipient used to encrypt the local cache. For file identities this is derived automatically.
    #[arg(long)]
    age_recipient: Option<String>,
    /// age identity used to decrypt the local cache.
    #[arg(long, default_value = "~/.config/rage/key.txt")]
    age_identity: String,
    /// Where to read the age identity from.
    #[arg(long, value_enum, default_value = "file")]
    age_identity_source: IdentitySource,
    /// macOS Keychain service name for --age-identity-source keychain.
    #[arg(long)]
    keychain_service: Option<String>,
    /// macOS Keychain account name for --age-identity-source keychain.
    #[arg(long)]
    keychain_account: Option<String>,
    /// Prefix for generated local cache file names.
    #[arg(long, default_value = DEFAULT_SECRET_PREFIX)]
    secret_prefix: String,
    /// Override the encrypted cache directory.
    #[arg(long)]
    cache_dir: Option<String>,
}

#[derive(Args)]
struct SetArgs {
    bundle: String,
    key: String,
    value: String,
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
}

#[derive(Args)]
struct KeyArgs {
    bundle: String,
    key: String,
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
    /// Print shell code that unsets the key in the current shell after updating storage.
    #[arg(long, hide = true)]
    emit_shell_unset: bool,
}

#[derive(Args)]
struct ListArgs {
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
}

#[derive(Args)]
struct DeleteBundleArgs {
    bundle: String,
    /// Confirm deletion of the remote GCP Secret Manager secret and local cache.
    #[arg(long)]
    yes: bool,
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
}

#[derive(Args)]
struct GetArgs {
    bundle: String,
    key: Option<String>,
    /// Fetch this bundle before reading it.
    #[arg(long)]
    sync: bool,
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
}

#[derive(Args)]
struct SyncArgs {
    /// Bundles to sync. If omitted with --all, every rage bundle in GCP Secret Manager is synced.
    bundles: Vec<String>,
    /// Sync every remote rage bundle.
    #[arg(long)]
    all: bool,
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
}

#[derive(Args)]
struct LoadArgs {
    bundles: Vec<String>,
    /// Fetch missing or stale bundles before loading them.
    #[arg(long)]
    sync: bool,
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
    /// Output format.
    #[arg(long, value_enum, default_value = "export")]
    format: LoadFormat,
    /// Do not print the shell hook that lets sourced `rage load` update the current shell on `rage unset`.
    #[arg(long)]
    no_shell_hook: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum LoadFormat {
    Export,
    Dotenv,
    Json,
}

#[derive(Args)]
struct ExecArgs {
    bundles: Vec<String>,
    /// Fetch bundles before running the command.
    #[arg(long)]
    sync: bool,
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
    /// Command to run, after --.
    #[arg(last = true, required = true)]
    command: Vec<OsString>,
}

#[derive(Args)]
struct ShellArgs {
    bundles: Vec<String>,
    /// Fetch bundles before starting the shell.
    #[arg(long)]
    sync: bool,
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
    /// Kill the shell after this many seconds.
    #[arg(long)]
    ttl_seconds: Option<u64>,
    /// Shell executable. Defaults to $SHELL or /bin/zsh.
    #[arg(long)]
    shell: Option<OsString>,
}

#[derive(Args)]
struct SshArgs {
    /// SSH host, alias, or user@host.
    host: String,
    /// Bundles to forward to the remote process.
    bundles: Vec<String>,
    /// Fetch bundles before opening SSH.
    #[arg(long)]
    sync: bool,
    /// Permit reading an age identity from macOS Keychain in an SSH session.
    #[arg(long)]
    allow_ssh_keychain: bool,
    /// Remote command to run, after --. Defaults to a login shell.
    #[arg(last = true)]
    command: Vec<OsString>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    gcp_project: String,
    #[serde(default = "default_gcp_endpoint")]
    gcp_endpoint: String,
    age_recipient: String,
    age_identity: String,
    #[serde(default)]
    age_identity_source: IdentitySource,
    #[serde(default)]
    keychain_service: Option<String>,
    #[serde(default)]
    keychain_account: Option<String>,
    secret_prefix: String,
    cache_dir: String,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    gcp_project: Option<String>,
    #[serde(default)]
    infisical_project_id: Option<String>,
    #[serde(default)]
    infisical_environment: Option<String>,
    #[serde(default)]
    infisical_endpoint: Option<String>,
    #[serde(default = "default_gcp_endpoint")]
    gcp_endpoint: String,
    age_recipient: String,
    age_identity: String,
    #[serde(default)]
    age_identity_source: IdentitySource,
    #[serde(default)]
    keychain_service: Option<String>,
    #[serde(default)]
    keychain_account: Option<String>,
    #[serde(default = "default_secret_prefix")]
    secret_prefix: String,
    #[serde(default = "default_cache_dir")]
    cache_dir: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
enum IdentitySource {
    #[default]
    File,
    Keychain,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init(args) => init(args),
        Commands::Auth(args) => auth(args),
        Commands::Import(args) => {
            let cfg = Config::load()?;
            agent_auth::import(args, &cfg)
        }
        Commands::Config => {
            let cfg = Config::load()?;
            println!("{}", toml::to_string_pretty(&cfg)?);
            Ok(())
        }
        Commands::List(args) => {
            let cfg = Config::load()?;
            for bundle in remote_list_bundles(&cfg, args.allow_ssh_keychain)? {
                println!("{bundle}");
            }
            Ok(())
        }
        Commands::Set(args) => {
            validate_bundle_key(&args.key)?;
            let cfg = Config::load()?;
            let mut env = remote_read_bundle(&cfg, &args.bundle, args.allow_ssh_keychain)?
                .unwrap_or_default();
            env.insert(args.key, args.value);
            remote_write_bundle(&cfg, &args.bundle, &env, args.allow_ssh_keychain)?;
            write_cache(&cfg, &args.bundle, &env)?;
            println!("updated {}", args.bundle);
            Ok(())
        }
        Commands::Unset(args) => {
            validate_bundle_key(&args.key)?;
            let cfg = Config::load()?;
            let mut env = remote_read_bundle(&cfg, &args.bundle, args.allow_ssh_keychain)?
                .with_context(|| format!("remote bundle '{}' does not exist", args.bundle))?;
            env.remove(&args.key);
            remote_write_bundle(&cfg, &args.bundle, &env, args.allow_ssh_keychain)?;
            write_cache(&cfg, &args.bundle, &env)?;
            if args.emit_shell_unset {
                print_shell_unset(&args.bundle, &args.key);
            } else {
                println!("updated {}", args.bundle);
            }
            Ok(())
        }
        Commands::DeleteBundle(args) => {
            if !args.yes {
                bail!("refusing to delete without --yes");
            }
            let cfg = Config::load()?;
            remote_delete_bundle(&cfg, &args.bundle, args.allow_ssh_keychain)?;
            let cache_path = cache_path(&cfg, &args.bundle);
            if cache_path.exists() {
                fs::remove_file(&cache_path)
                    .with_context(|| format!("remove {}", cache_path.display()))?;
            }
            println!("deleted {}", args.bundle);
            Ok(())
        }
        Commands::Get(args) => {
            let cfg = Config::load()?;
            if args.sync {
                sync_bundle(&cfg, &args.bundle, args.allow_ssh_keychain)?;
            }
            let env = read_cache(&cfg, &args.bundle, args.allow_ssh_keychain)?;
            if let Some(key) = args.key {
                let value = env
                    .get(&key)
                    .with_context(|| format!("{} is not set in {}", key, args.bundle))?;
                println!("{value}");
            } else {
                print_dotenv(&env);
            }
            Ok(())
        }
        Commands::Sync(args) => {
            let cfg = Config::load()?;
            let allow_ssh_keychain = args.allow_ssh_keychain;
            let bundles = resolve_sync_bundles(&cfg, args)?;
            for bundle in bundles {
                sync_bundle(&cfg, &bundle, allow_ssh_keychain)?;
                println!("synced {bundle}");
            }
            Ok(())
        }
        Commands::Load(args) => {
            let cfg = Config::load()?;
            let env = load_env(&cfg, &args.bundles, args.sync, args.allow_ssh_keychain)?;
            match args.format {
                LoadFormat::Export => print_exports(&env, !args.no_shell_hook)?,
                LoadFormat::Dotenv => print_dotenv(&env),
                LoadFormat::Json => print_json(&env),
            }
            Ok(())
        }
        Commands::Exec(args) => {
            let cfg = Config::load()?;
            let env = load_env(&cfg, &args.bundles, args.sync, args.allow_ssh_keychain)?;
            run_command(args.command, env, None)
        }
        Commands::Shell(args) => {
            let cfg = Config::load()?;
            let env = load_env(&cfg, &args.bundles, args.sync, args.allow_ssh_keychain)?;
            let shell = args
                .shell
                .or_else(|| std::env::var_os("SHELL"))
                .unwrap_or_else(|| OsString::from("/bin/zsh"));
            run_command(vec![shell, OsString::from("-l")], env, args.ttl_seconds)
        }
        Commands::Ssh(args) => {
            let cfg = Config::load()?;
            let env = load_env(&cfg, &args.bundles, args.sync, args.allow_ssh_keychain)?;
            run_ssh(args.host, env, args.command)
        }
        Commands::Grok(args) => {
            let cfg = Config::load()?;
            agent_auth::run_grok(args, &cfg)
        }
        Commands::Codex(args) => {
            let cfg = Config::load()?;
            agent_auth::run_codex(args, &cfg)
        }
        Commands::Tui(args) => {
            let cfg = Config::load()?;
            tui::run(&cfg, args.allow_ssh_keychain)
        }
    }
}

fn init(args: InitArgs) -> Result<()> {
    validate_secret_prefix(&args.secret_prefix)?;
    let cache_dir = args.cache_dir.clone().unwrap_or_else(default_cache_dir);
    let age_recipient = resolve_init_recipient(&args)?;
    let gcp_endpoint = env_gcp_endpoint(&args.gcp_endpoint);
    let gcp_project = resolve_gcp_project(args.gcp_project.or(args.infisical_project_id))?;
    let cfg = Config {
        gcp_project,
        gcp_endpoint,
        age_recipient,
        age_identity: args.age_identity,
        age_identity_source: args.age_identity_source,
        keychain_service: args.keychain_service,
        keychain_account: args.keychain_account,
        secret_prefix: args.secret_prefix,
        cache_dir,
    };
    validate_identity_config(&cfg)?;

    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, toml::to_string_pretty(&cfg)?)?;
    fs::create_dir_all(expand_tilde(&cfg.cache_dir))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn auth(args: AuthArgs) -> Result<()> {
    match args.command {
        AuthCommand::Status => {
            if gcp_access_token().is_ok() {
                println!("auth: gcp-access-token-env");
            } else {
                println!("auth: not-configured");
            }
            Ok(())
        }
    }
}

fn resolve_init_recipient(args: &InitArgs) -> Result<String> {
    match args.age_identity_source {
        IdentitySource::File => {
            let identity_path = expand_tilde(&args.age_identity);
            if !identity_path.exists() {
                generate_age_identity_file(&identity_path)?;
            }
            let derived = recipient_from_identity_file(&identity_path)?;
            if let Some(explicit) = &args.age_recipient
                && explicit != &derived
            {
                bail!(
                    "--age-recipient does not match the public recipient derived from {}",
                    identity_path.display()
                );
            }
            Ok(derived)
        }
        IdentitySource::Keychain => args.age_recipient.clone().ok_or_else(|| {
            anyhow!("--age-recipient is required when --age-identity-source keychain is used")
        }),
    }
}

fn generate_age_identity_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let identity = age::x25519::Identity::generate();
    let contents = format!("{}\n", identity.to_string().expose_secret());
    fs::write(path, contents)
        .with_context(|| format!("write generated age identity at {}", path.display()))?;
    set_owner_read_write_only(path)?;
    Ok(())
}

fn recipient_from_identity_file(path: &Path) -> Result<String> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read age identity at {}", path.display()))?;
    recipient_from_identity_text(&raw)
}

fn recipient_from_identity_text(raw: &str) -> Result<String> {
    let identity = parse_age_identity(raw)?;
    Ok(identity.to_public().to_string())
}

fn parse_age_identity(raw: &str) -> Result<age::x25519::Identity> {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return age::x25519::Identity::from_str(trimmed).map_err(|err| anyhow!(err));
    }
    bail!("age identity is empty")
}

fn encrypt_age(recipient: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
    let recipient = age::x25519::Recipient::from_str(recipient)
        .map_err(|err| anyhow!("invalid age recipient: {err}"))?;
    let encryptor = age::Encryptor::with_recipients(std::iter::once(&recipient as _))?;
    let mut encrypted = Vec::new();
    let mut writer = encryptor.wrap_output(&mut encrypted)?;
    writer.write_all(plaintext)?;
    writer.finish()?;
    Ok(encrypted)
}

fn decrypt_age(identity: &str, encrypted: &[u8]) -> Result<Vec<u8>> {
    let identity = parse_age_identity(identity)?;
    let decryptor = age::Decryptor::new(encrypted)?;
    let mut reader = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))?;
    let mut plaintext = Vec::new();
    reader.read_to_end(&mut plaintext)?;
    Ok(plaintext)
}

#[cfg(unix)]
fn set_owner_read_write_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restrict permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_owner_read_write_only(_path: &Path) -> Result<()> {
    Ok(())
}

impl Config {
    fn load() -> Result<Self> {
        let path = config_path()?;
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("read config at {}", path.display()))?;
        let raw_cfg: RawConfig = toml::from_str(&raw)?;
        let should_rewrite = raw_cfg.infisical_project_id.is_some()
            || raw_cfg.infisical_environment.is_some()
            || raw_cfg.infisical_endpoint.is_some()
            || raw_cfg
                .gcp_project
                .as_deref()
                .is_none_or(|project| project.trim().is_empty());
        let cfg = Config::from_raw(raw_cfg)?;
        if cfg.gcp_project.trim().is_empty() {
            bail!("gcp_project is required in config");
        }
        validate_secret_prefix(&cfg.secret_prefix)?;
        validate_identity_config(&cfg)?;
        if should_rewrite {
            fs::write(&path, toml::to_string_pretty(&cfg)?)
                .with_context(|| format!("write migrated config at {}", path.display()))?;
        }
        Ok(cfg)
    }

    fn from_raw(raw: RawConfig) -> Result<Self> {
        let gcp_project = resolve_gcp_project(raw.gcp_project.or(raw.infisical_project_id))?;
        Ok(Self {
            gcp_project,
            gcp_endpoint: env_gcp_endpoint(&raw.gcp_endpoint),
            age_recipient: raw.age_recipient,
            age_identity: raw.age_identity,
            age_identity_source: raw.age_identity_source,
            keychain_service: raw.keychain_service,
            keychain_account: raw.keychain_account,
            secret_prefix: raw.secret_prefix,
            cache_dir: raw.cache_dir,
        })
    }
}

fn default_gcp_endpoint() -> String {
    DEFAULT_GCP_ENDPOINT.to_string()
}

fn default_secret_prefix() -> String {
    DEFAULT_SECRET_PREFIX.to_string()
}

fn normalize_endpoint(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

fn env_gcp_endpoint(default: &str) -> String {
    std::env::var("RAGE_GCP_ENDPOINT")
        .map(|value| normalize_endpoint(&value))
        .unwrap_or_else(|_| normalize_endpoint(default))
}

fn resolve_gcp_project(explicit: Option<String>) -> Result<String> {
    explicit
        .or_else(gcp_project_from_env)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow!(
                "gcp_project is required; pass --gcp-project or set RAGE_GCP_PROJECT/GOOGLE_CLOUD_PROJECT"
            )
        })
}

fn gcp_project_from_env() -> Option<String> {
    std::env::var("RAGE_GCP_PROJECT")
        .or_else(|_| std::env::var("GOOGLE_CLOUD_PROJECT"))
        .or_else(|_| std::env::var("GOOGLE_PROJECT_ID"))
        .or_else(|_| std::env::var("GCLOUD_PROJECT"))
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn config_path() -> Result<PathBuf> {
    let base = if let Some(dir) = std::env::var_os("RAGE_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::config_dir().ok_or_else(|| anyhow!("could not determine config directory"))?
    };
    Ok(base.join("rage").join(CONFIG_FILE_NAME))
}

fn default_cache_dir() -> String {
    if let Some(dir) = std::env::var_os("RAGE_CACHE_DIR") {
        return PathBuf::from(dir).to_string_lossy().into_owned();
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("~/.cache"))
        .join("rage")
        .to_string_lossy()
        .into_owned()
}

fn resolve_sync_bundles(cfg: &Config, args: SyncArgs) -> Result<Vec<String>> {
    match (args.all, args.bundles.is_empty()) {
        (true, true) => remote_list_bundles(cfg, args.allow_ssh_keychain),
        (false, false) => Ok(args.bundles),
        (true, false) => bail!("pass either explicit bundles or --all, not both"),
        (false, true) => bail!("pass one or more bundles, or use --all"),
    }
}

fn load_env(
    cfg: &Config,
    bundles: &[String],
    sync_first: bool,
    allow_ssh_keychain: bool,
) -> Result<BTreeMap<String, String>> {
    if bundles.is_empty() {
        bail!("pass at least one bundle");
    }

    let mut merged = BTreeMap::new();
    for bundle in bundles {
        if sync_first {
            sync_bundle(cfg, bundle, allow_ssh_keychain)?;
        }
        for (key, value) in read_cache(cfg, bundle, allow_ssh_keychain)? {
            merged.insert(key, value);
        }
    }
    Ok(merged)
}

fn sync_bundle(cfg: &Config, bundle: &str, allow_ssh_keychain: bool) -> Result<()> {
    let env = remote_read_bundle(cfg, bundle, allow_ssh_keychain)?
        .with_context(|| format!("remote bundle '{}' does not exist", bundle))?;
    write_cache(cfg, bundle, &env)
}

pub(crate) fn remote_read_bundle(
    cfg: &Config,
    bundle: &str,
    allow_ssh_keychain: bool,
) -> Result<Option<BTreeMap<String, String>>> {
    let client = GcpSecretManagerClient::new(cfg, allow_ssh_keychain)?;
    let Some(raw) = client.read_bundle(bundle)? else {
        if bundle == AGENT_AUTH_BUNDLE {
            let legacy_agent_auth_exists = client
                .read_bundle("global")?
                .unwrap_or_default()
                .keys()
                .any(|key| is_reserved_remote_key(key));
            if legacy_agent_auth_exists {
                return Ok(Some(BTreeMap::new()));
            }
        }
        return Ok(None);
    };
    let mut env = BTreeMap::new();
    let mut reserved_exists = false;
    for (key, value) in raw {
        if is_reserved_remote_key(&key) {
            reserved_exists = true;
            continue;
        }
        validate_env_key(&key)?;
        env.insert(key, value);
    }
    if env.is_empty() && !reserved_exists {
        return Ok(None);
    }
    Ok(Some(env))
}

pub(crate) fn remote_read_bundle_for_display(
    cfg: &Config,
    bundle: &str,
    allow_ssh_keychain: bool,
) -> Result<Option<BTreeMap<String, String>>> {
    let client = GcpSecretManagerClient::new(cfg, allow_ssh_keychain)?;
    let raw = client.read_bundle(bundle)?.unwrap_or_default();
    let mut env = BTreeMap::new();

    for (key, value) in raw {
        if is_reserved_remote_key(&key) {
            if bundle == AGENT_AUTH_BUNDLE {
                env.insert(key, AGENT_AUTH_DISPLAY_VALUE.to_string());
            }
            continue;
        }
        validate_env_key(&key)?;
        env.insert(key, value);
    }

    if bundle == AGENT_AUTH_BUNDLE {
        for (key, _) in client.read_bundle("global")?.unwrap_or_default() {
            if is_reserved_remote_key(&key) {
                env.entry(key)
                    .or_insert_with(|| AGENT_AUTH_DISPLAY_VALUE.to_string());
            }
        }
    }

    if env.is_empty() {
        return Ok(None);
    }
    Ok(Some(env))
}

pub(crate) fn remote_write_bundle(
    cfg: &Config,
    bundle: &str,
    env: &BTreeMap<String, String>,
    allow_ssh_keychain: bool,
) -> Result<()> {
    let client = GcpSecretManagerClient::new(cfg, allow_ssh_keychain)?;
    let mut merged = client.read_bundle(bundle)?.unwrap_or_default();
    merged.retain(|key, _| is_reserved_remote_key(key));
    for (key, value) in env {
        merged.insert(key.clone(), value.clone());
    }
    client.write_bundle(bundle, &merged)?;
    Ok(())
}

pub(crate) fn remote_delete_bundle(
    cfg: &Config,
    bundle: &str,
    allow_ssh_keychain: bool,
) -> Result<()> {
    let client = GcpSecretManagerClient::new(cfg, allow_ssh_keychain)?;
    let mut reserved = client.read_bundle(bundle)?.unwrap_or_default();
    reserved.retain(|key, _| is_reserved_remote_key(key));
    if reserved.is_empty() {
        client.delete_bundle(bundle)?;
    } else {
        client.write_bundle(bundle, &reserved)?;
    }
    Ok(())
}

pub(crate) fn remote_list_bundles(cfg: &Config, allow_ssh_keychain: bool) -> Result<Vec<String>> {
    let client = GcpSecretManagerClient::new(cfg, allow_ssh_keychain)?;
    let mut bundles = Vec::new();
    for bundle in client.list_bundles()? {
        if bundle == "authless" || bundle.starts_with("authless/") {
            continue;
        }
        let raw = client.read_bundle(&bundle)?.unwrap_or_default();
        let has_public_keys = raw.keys().any(|key| !is_reserved_remote_key(key));
        let has_reserved_keys = raw.keys().any(|key| is_reserved_remote_key(key));
        if has_public_keys || bundle == AGENT_AUTH_BUNDLE {
            bundles.push(bundle.clone());
        }
        if bundle == "global" && has_reserved_keys {
            bundles.push(AGENT_AUTH_BUNDLE.to_string());
        }
    }
    bundles.sort();
    bundles.dedup();
    Ok(bundles)
}

fn bundle_from_secret_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "global".to_string();
    }
    trimmed.trim_matches('/').to_string()
}

fn url_component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

pub(crate) fn read_remote_auth_record(cfg: &Config, provider: &str) -> Result<String> {
    let key = auth_secret_key(provider);
    if let Some(raw) = read_remote_secret(cfg, AGENT_AUTH_SECRET_PATH, &key)? {
        return Ok(raw);
    }
    if let Some(raw) = read_remote_secret(cfg, "/", &key)? {
        write_remote_auth_record(cfg, provider, &raw)?;
        return Ok(raw);
    }
    bail!("{provider} auth is not imported; run `rage import {provider} <auth-file>`")
}

pub(crate) fn write_remote_auth_record(cfg: &Config, provider: &str, raw: &str) -> Result<()> {
    write_remote_secret(cfg, AGENT_AUTH_SECRET_PATH, &auth_secret_key(provider), raw)
}

fn auth_secret_key(provider: &str) -> String {
    format!("AUTHLESS_{}_JSON", provider.to_ascii_uppercase())
}

pub(crate) fn is_reserved_agent_auth_key(key: &str) -> bool {
    is_reserved_remote_key(key)
}

fn is_reserved_remote_key(key: &str) -> bool {
    key.starts_with("AUTHLESS_") && key.ends_with("_JSON")
}

pub(crate) fn read_remote_secret(
    cfg: &Config,
    secret_path: &str,
    key: &str,
) -> Result<Option<String>> {
    let client = GcpSecretManagerClient::new(cfg, false)?;
    let bundle = bundle_from_secret_path(secret_path);
    Ok(client
        .read_bundle(&bundle)?
        .and_then(|bundle| bundle.get(key).cloned()))
}

pub(crate) fn write_remote_secret(
    cfg: &Config,
    secret_path: &str,
    key: &str,
    value: &str,
) -> Result<()> {
    let client = GcpSecretManagerClient::new(cfg, false)?;
    let bundle = bundle_from_secret_path(secret_path);
    let mut env = client.read_bundle(&bundle)?.unwrap_or_default();
    env.insert(key.to_string(), value.to_string());
    client.write_bundle(&bundle, &env)
}

struct GcpSecretManagerClient {
    http: Client,
    project: String,
    endpoint: String,
    token: String,
    secret_prefix: String,
}

impl GcpSecretManagerClient {
    fn new(cfg: &Config, allow_ssh_keychain: bool) -> Result<Self> {
        let _ = identity_text(cfg, allow_ssh_keychain)?;
        let http = Client::new();
        let endpoint = env_gcp_endpoint(&cfg.gcp_endpoint);
        let token = gcp_access_token()?;
        Ok(Self {
            http,
            project: gcp_project_from_env().unwrap_or_else(|| cfg.gcp_project.clone()),
            endpoint,
            token,
            secret_prefix: cfg.secret_prefix.clone(),
        })
    }

    fn project_url(&self) -> String {
        format!(
            "{}/v1/projects/{}",
            self.endpoint,
            url_component(&self.project)
        )
    }

    fn secret_url(&self, secret_id: &str) -> String {
        format!(
            "{}/secrets/{}",
            self.project_url(),
            url_component(secret_id)
        )
    }

    fn bundle_secret_id(&self, bundle: &str) -> String {
        format!("{}-{}", self.secret_prefix, encode_bundle(bundle))
    }

    fn list_bundles(&self) -> Result<Vec<String>> {
        let mut bundles = Vec::new();
        let mut page_token = String::new();
        loop {
            let mut request = self
                .http
                .get(format!("{}/secrets", self.project_url()))
                .bearer_auth(&self.token);
            if !page_token.is_empty() {
                request = request.query(&[("pageToken", page_token.as_str())]);
            }
            let response = request.send().context("list GCP Secret Manager secrets")?;
            if !response.status().is_success() {
                return Err(gcp_response_error("list secrets", response));
            }
            let body: GcpListSecretsResponse = response
                .json()
                .context("parse GCP Secret Manager list response")?;
            for secret in body.secrets {
                if let Some(bundle) = self.bundle_from_secret_name(&secret.name) {
                    bundles.push(bundle);
                }
            }
            page_token = body.next_page_token.unwrap_or_default();
            if page_token.is_empty() {
                break;
            }
        }
        Ok(bundles)
    }

    fn read_bundle(&self, bundle: &str) -> Result<Option<BTreeMap<String, String>>> {
        let secret_id = self.bundle_secret_id(bundle);
        let response = self
            .http
            .get(format!(
                "{}/versions/latest:access",
                self.secret_url(&secret_id)
            ))
            .bearer_auth(&self.token)
            .send()
            .with_context(|| format!("access GCP Secret Manager secret {secret_id}"))?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(gcp_response_error("access secret", response));
        }
        let body: GcpAccessSecretResponse = response
            .json()
            .context("parse GCP Secret Manager access response")?;
        let data = STANDARD
            .decode(body.payload.data.as_bytes())
            .context("decode GCP Secret Manager payload")?;
        parse_dotenv(&String::from_utf8(data)?).map(Some)
    }

    fn write_bundle(&self, bundle: &str, env: &BTreeMap<String, String>) -> Result<()> {
        let secret_id = self.bundle_secret_id(bundle);
        self.ensure_secret(&secret_id)?;
        let payload = STANDARD.encode(render_dotenv(env).as_bytes());
        let response = self
            .http
            .post(format!("{}:addVersion", self.secret_url(&secret_id)))
            .bearer_auth(&self.token)
            .json(&GcpAddVersionRequest {
                payload: GcpPayload { data: &payload },
            })
            .send()
            .with_context(|| format!("add GCP Secret Manager version for {secret_id}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(gcp_response_error("add secret version", response))
        }
    }

    fn delete_bundle(&self, bundle: &str) -> Result<()> {
        let secret_id = self.bundle_secret_id(bundle);
        let response = self
            .http
            .delete(self.secret_url(&secret_id))
            .bearer_auth(&self.token)
            .send()
            .with_context(|| format!("delete GCP Secret Manager secret {secret_id}"))?;
        if response.status().is_success() || response.status().as_u16() == 404 {
            Ok(())
        } else {
            Err(gcp_response_error("delete secret", response))
        }
    }

    fn ensure_secret(&self, secret_id: &str) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/secrets", self.project_url()))
            .bearer_auth(&self.token)
            .query(&[("secretId", secret_id)])
            .json(&GcpCreateSecretRequest {
                replication: GcpReplication {
                    automatic: BTreeMap::new(),
                },
            })
            .send()
            .with_context(|| format!("create GCP Secret Manager secret {secret_id}"))?;
        if response.status().is_success() || response.status().as_u16() == 409 {
            Ok(())
        } else {
            Err(gcp_response_error("create secret", response))
        }
    }

    fn bundle_from_secret_name(&self, name: &str) -> Option<String> {
        let secret_id = name.rsplit_once("/secrets/")?.1;
        let encoded = secret_id.strip_prefix(&format!("{}-", self.secret_prefix))?;
        let bytes = URL_SAFE_NO_PAD.decode(encoded.as_bytes()).ok()?;
        String::from_utf8(bytes).ok()
    }
}

fn gcp_response_error(action: &str, response: reqwest::blocking::Response) -> anyhow::Error {
    let status = response.status();
    let body = response
        .text()
        .unwrap_or_else(|_| "<unreadable body>".to_string());
    anyhow!("GCP Secret Manager {action} failed with {status}: {body}")
}

fn gcp_access_token() -> Result<String> {
    match std::env::var("GCP_ACCESS_TOKEN")
        .or_else(|_| std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN"))
        .or_else(|_| std::env::var("CLOUDSDK_AUTH_ACCESS_TOKEN"))
    {
        Ok(token) if !token.trim().is_empty() => Ok(token),
        _ => bail!(
            "GCP auth is not configured; set GCP_ACCESS_TOKEN or GOOGLE_OAUTH_ACCESS_TOKEN with Secret Manager access"
        ),
    }
}

fn identity_text(cfg: &Config, allow_ssh_keychain: bool) -> Result<String> {
    match cfg.age_identity_source {
        IdentitySource::File => fs::read_to_string(expand_tilde(&cfg.age_identity))
            .with_context(|| format!("read age identity at {}", cfg.age_identity)),
        IdentitySource::Keychain => keychain_identity(cfg, allow_ssh_keychain),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GcpListSecretsResponse {
    #[serde(default)]
    secrets: Vec<GcpSecret>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct GcpSecret {
    name: String,
}

#[derive(Deserialize)]
struct GcpAccessSecretResponse {
    payload: GcpOwnedPayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GcpOwnedPayload {
    data: String,
}

#[derive(Serialize)]
struct GcpCreateSecretRequest {
    replication: GcpReplication,
}

#[derive(Serialize)]
struct GcpReplication {
    automatic: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct GcpAddVersionRequest<'a> {
    payload: GcpPayload<'a>,
}

#[derive(Serialize)]
struct GcpPayload<'a> {
    data: &'a str,
}

fn write_cache(cfg: &Config, bundle: &str, env: &BTreeMap<String, String>) -> Result<()> {
    let cache_path = cache_path(cfg, bundle);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let payload = render_dotenv(env);
    let encrypted = encrypt_age(&cfg.age_recipient, payload.as_bytes())?;
    fs::write(&cache_path, encrypted)
        .with_context(|| format!("write encrypted cache at {}", cache_path.display()))?;
    Ok(())
}

fn read_cache(
    cfg: &Config,
    bundle: &str,
    allow_ssh_keychain: bool,
) -> Result<BTreeMap<String, String>> {
    let cache_path = cache_path(cfg, bundle);
    let identity = identity_text(cfg, allow_ssh_keychain)?;
    let encrypted = fs::read(&cache_path)
        .with_context(|| format!("read encrypted cache at {}", cache_path.display()))?;
    let plaintext = decrypt_age(&identity, &encrypted)
        .with_context(|| format!("decrypt encrypted cache for {}", bundle))?;
    parse_dotenv(&String::from_utf8(plaintext)?)
}

fn keychain_identity(cfg: &Config, allow_ssh_keychain: bool) -> Result<String> {
    if is_ssh_session() && !allow_ssh_keychain {
        bail!(
            "refusing to read macOS Keychain identity from an SSH session; pass --allow-ssh-keychain to opt in"
        );
    }

    let service = cfg
        .keychain_service
        .as_deref()
        .context("keychain_service is required for keychain identity source")?;
    let account = cfg.keychain_account.as_deref().unwrap_or(&cfg.age_identity);
    let output = Command::new("security")
        .args(["find-generic-password", "-w", "-s", service, "-a", account])
        .output()
        .with_context(|| "run security find-generic-password")?;
    if !output.status.success() {
        bail!(
            "failed reading age identity from macOS Keychain service '{}', account '{}': {}",
            service,
            account,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).map_err(Into::into)
}

fn cache_path(cfg: &Config, bundle: &str) -> PathBuf {
    expand_tilde(&cfg.cache_dir).join(format!("{}.env.age", secret_id(cfg, bundle)))
}

fn secret_id(cfg: &Config, bundle: &str) -> String {
    format!("{}-{}", cfg.secret_prefix, encode_bundle(bundle))
}

fn encode_bundle(bundle: &str) -> String {
    URL_SAFE_NO_PAD.encode(bundle.as_bytes())
}

fn parse_dotenv(raw: &str) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    for item in dotenvy::from_read_iter(raw.as_bytes()) {
        let (key, value) = item?;
        validate_env_key(&key)?;
        env.insert(key, value);
    }
    Ok(env)
}

fn render_dotenv(env: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (key, value) in env {
        out.push_str(key);
        out.push('=');
        out.push_str(&quote_dotenv(value));
        out.push('\n');
    }
    out
}

fn quote_dotenv(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | '@'))
    {
        return value.to_string();
    }
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn print_exports(env: &BTreeMap<String, String>, include_shell_hook: bool) -> Result<()> {
    for (key, value) in env {
        println!("export {key}={}", quote_shell(value));
    }
    if include_shell_hook {
        print_shell_hook()?;
    }
    Ok(())
}

fn print_shell_unset(bundle: &str, key: &str) {
    println!("unset {key}");
    println!(
        "printf '%s\\n' {}",
        quote_shell(&format!("updated {bundle}"))
    );
}

fn print_shell_hook() -> Result<()> {
    let rage_bin = std::env::current_exe().context("resolve current rage executable")?;
    println!("__rage_bin={}", quote_shell(&rage_bin.to_string_lossy()));
    println!("rage() {{");
    println!("  if [ \"$#\" -ge 3 ] && [ \"$1\" = 'unset' ]; then");
    println!("    __rage_script=\"$(\"$__rage_bin\" \"$@\" --emit-shell-unset)\"");
    println!("    __rage_status=$?");
    println!("    if [ \"$__rage_status\" -eq 0 ]; then");
    println!("      eval \"$__rage_script\"");
    println!("      __rage_status=$?");
    println!("    fi");
    println!("    __rage_return=$__rage_status");
    println!("    unset __rage_script __rage_status");
    println!("    return \"$__rage_return\"");
    println!("  fi");
    println!("  \"$__rage_bin\" \"$@\"");
    println!("}}");
    Ok(())
}

fn print_dotenv(env: &BTreeMap<String, String>) {
    print!("{}", render_dotenv(env));
}

fn print_json(env: &BTreeMap<String, String>) {
    println!("{{");
    for (idx, (key, value)) in env.iter().enumerate() {
        let comma = if idx + 1 == env.len() { "" } else { "," };
        println!(
            "  \"{}\": \"{}\"{}",
            escape_json(key),
            escape_json(value),
            comma
        );
    }
    println!("}}");
}

fn quote_shell(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let mut out = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn escape_json(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn run_command(
    command: Vec<OsString>,
    env: BTreeMap<String, String>,
    ttl_seconds: Option<u64>,
) -> Result<()> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("missing command"))?;
    let mut child = Command::new(program)
        .args(args)
        .envs(env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("start {:?}", program))?;

    let status = if let Some(ttl) = ttl_seconds {
        wait_with_timeout(&mut child, Duration::from_secs(ttl))?
    } else {
        child.wait()?
    };

    if status.success() {
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn run_ssh(host: String, env: BTreeMap<String, String>, command: Vec<OsString>) -> Result<()> {
    let script = remote_script(&env, &command);
    let mut child = Command::new("ssh")
        .arg(host)
        .arg("sh -s")
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| "start ssh")?;

    child
        .stdin
        .as_mut()
        .context("open ssh stdin")?
        .write_all(script.as_bytes())?;

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn remote_script(env: &BTreeMap<String, String>, command: &[OsString]) -> String {
    let mut script = String::from("set -eu\n");
    for (key, value) in env {
        script.push_str("export ");
        script.push_str(key);
        script.push('=');
        script.push_str(&quote_shell(value));
        script.push('\n');
    }

    if command.is_empty() {
        script.push_str("exec \"${SHELL:-/bin/sh}\" -l\n");
    } else {
        script.push_str("exec");
        for arg in command {
            script.push(' ');
            script.push_str(&quote_shell(&arg.to_string_lossy()));
        }
        script.push('\n');
    }
    script
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    ttl: Duration,
) -> Result<std::process::ExitStatus> {
    let deadline = Instant::now() + ttl;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            return child.wait().context("wait for killed child");
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn validate_secret_prefix(prefix: &str) -> Result<()> {
    if prefix.is_empty() {
        bail!("secret prefix cannot be empty");
    }
    if !prefix
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        bail!("secret prefix may contain only letters, numbers, hyphens, and underscores");
    }
    Ok(())
}

fn validate_identity_config(cfg: &Config) -> Result<()> {
    if cfg.age_identity_source == IdentitySource::Keychain && cfg.keychain_service.is_none() {
        bail!("--keychain-service is required when --age-identity-source keychain is used");
    }
    Ok(())
}

fn is_ssh_session() -> bool {
    std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some()
}

fn validate_env_key(key: &str) -> Result<()> {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        bail!("environment key cannot be empty");
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        bail!("environment key must start with a letter or underscore");
    }
    if !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        bail!("environment key may contain only letters, numbers, and underscores");
    }
    Ok(())
}

pub(crate) fn validate_bundle_key(key: &str) -> Result<()> {
    validate_env_key(key)?;
    if is_reserved_remote_key(key) {
        bail!("environment key name is reserved for rage agent auth");
    }
    Ok(())
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(rest);
    }
    Path::new(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_id_encodes_bundle_slashes() {
        let bundle = "project/foo/dev";
        assert_eq!(encode_bundle(bundle), "cHJvamVjdC9mb28vZGV2");
    }

    #[test]
    fn dotenv_render_parse_round_trip() {
        let mut env = BTreeMap::new();
        env.insert("PLAIN".to_string(), "abc-123".to_string());
        env.insert("QUOTED".to_string(), "hello world \"x\"".to_string());
        env.insert("MULTI".to_string(), "line1\nline2".to_string());
        assert_eq!(parse_dotenv(&render_dotenv(&env)).unwrap(), env);
    }

    #[test]
    fn shell_quotes_single_quotes() {
        assert_eq!(quote_shell("a'b"), "'a'\\''b'");
    }

    #[test]
    fn rejects_bad_env_keys() {
        assert!(validate_env_key("OK_1").is_ok());
        assert!(validate_env_key("1_BAD").is_err());
        assert!(validate_env_key("BAD-DASH").is_err());
    }

    #[test]
    fn merge_later_bundle_wins() {
        let mut a = BTreeMap::new();
        a.insert("KEY".to_string(), "a".to_string());
        let mut b = BTreeMap::new();
        b.insert("KEY".to_string(), "b".to_string());

        let mut merged = BTreeMap::new();
        for (key, value) in a {
            merged.insert(key, value);
        }
        for (key, value) in b {
            merged.insert(key, value);
        }
        assert_eq!(merged.get("KEY").unwrap(), "b");
    }

    #[test]
    fn json_escapes_control_chars() {
        assert_eq!(escape_json("a\n\"b\""), "a\\n\\\"b\\\"");
    }

    #[test]
    fn remote_script_uses_stdin_safe_shape() {
        let mut env = BTreeMap::new();
        env.insert("TOKEN".to_string(), "a b'c".to_string());
        let script = remote_script(&env, &[OsString::from("printf"), OsString::from("$TOKEN")]);
        assert!(script.contains("export TOKEN='a b'\\''c'"));
        assert!(script.contains("exec 'printf' '$TOKEN'"));
    }
}
