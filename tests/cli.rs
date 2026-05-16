use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use tempfile::TempDir;

struct TestHome {
    _tmp: TempDir,
    config_dir: PathBuf,
    cache_dir: PathBuf,
}

impl TestHome {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("config");
        let cache_dir = tmp.path().join("cache");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::create_dir_all(&cache_dir).expect("cache dir");
        Self {
            _tmp: tmp,
            config_dir,
            cache_dir,
        }
    }

    fn rage(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("rage").expect("rage binary");
        cmd.env("RAGE_CONFIG_DIR", &self.config_dir)
            .env("RAGE_CACHE_DIR", &self.cache_dir)
            .env_remove("INFISICAL_TOKEN")
            .env_remove("INFISICAL_MACHINE_IDENTITY_CLIENT_ID")
            .env_remove("INFISICAL_MACHINE_IDENTITY_CLIENT_SECRET")
            .env_remove("INFISICAL_UNIVERSAL_AUTH_CLIENT_ID")
            .env_remove("INFISICAL_UNIVERSAL_AUTH_CLIENT_SECRET")
            .env_remove("INFISICAL_ORGANIZATION_SLUG")
            .env_remove("INFISICAL_ORG_SLUG")
            .env_remove("INFISICAL_PROJECT_ID")
            .env_remove("INFISICAL_PROJECT_SLUG")
            .env_remove("INFISICAL_ENVIRONMENT")
            .env_remove("INFISICAL_API_URL")
            .env_remove("RAGE_INFISICAL_ENDPOINT")
            .env_remove("RAGE_INFISICAL_PROJECT_ID")
            .env_remove("RAGE_INFISICAL_PROJECT_SLUG")
            .env_remove("RAGE_INFISICAL_ENVIRONMENT")
            .env_remove("GROK_AUTH_ENDPOINT_URL")
            .env_remove("CODEX_AUTH_ENDPOINT_URL")
            .env_remove("CODEX_HOME");
        cmd
    }
}

#[test]
fn init_generates_file_identity_and_recipient_by_default() {
    let home = TestHome::new();
    let identity_path = home.config_dir.join("rage/key.txt");

    home.rage()
        .args([
            "init",
            "--infisical-project-id",
            "test-project",
            "--age-identity",
            identity_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let config = fs::read_to_string(home.config_dir.join("rage/config.toml")).unwrap();
    assert!(config.contains("infisical_project_id = \"test-project\""));
    assert!(config.contains("infisical_environment = \"prod\""));
    assert!(config.contains("age_identity_source = \"file\""));
    assert!(config.contains("age_recipient = \"age1"));
    assert!(config.contains("secret_prefix = \"rage\""));
    assert!(
        fs::read_to_string(identity_path)
            .unwrap()
            .contains("AGE-SECRET-KEY-")
    );
}

#[test]
fn init_can_infer_infisical_project_id_from_token_metadata() {
    let home = TestHome::new();
    let fake = FakeInfisical::new();
    let identity_path = home.config_dir.join("rage/key.txt");

    fake.apply(&mut home.rage())
        .args(["init", "--age-identity", identity_path.to_str().unwrap()])
        .assert()
        .success();

    let config = fs::read_to_string(home.config_dir.join("rage/config.toml")).unwrap();
    assert!(config.contains("infisical_project_id = \"test-project\""));
}

#[test]
fn init_uses_machine_identity_credentials_and_infers_single_project() {
    let home = TestHome::new();
    let fake = FakeInfisical::new();
    let identity_path = home.config_dir.join("rage/key.txt");

    fake.apply_machine_identity(&mut home.rage())
        .args(["init", "--age-identity", identity_path.to_str().unwrap()])
        .assert()
        .success();

    fake.apply_machine_identity(&mut home.rage())
        .args(["set", "global", "A", "one"])
        .assert()
        .success()
        .stdout("updated global\n");
}

#[test]
fn legacy_gcp_config_migrates_to_inferred_infisical_project_id() {
    let home = TestHome::new();
    let identity_path = init_file_identity(&home);
    let recipient = config_value(&home, "age_recipient");
    let config_path = home.config_dir.join("rage/config.toml");
    fs::write(
        &config_path,
        format!(
            r#"gcp_project = "humanlabs"
age_recipient = "{recipient}"
age_identity = "{}"
age_identity_source = "file"
secret_prefix = "rage"
cache_dir = "{}"
"#,
            identity_path.display(),
            home.cache_dir.display()
        ),
    )
    .unwrap();

    let fake = FakeInfisical::new();
    let stdout = fake
        .apply_machine_identity(&mut home.rage())
        .arg("config")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(stdout).unwrap();
    assert!(stdout.contains("infisical_project_id = \"test-project\""));
    assert!(!stdout.contains("gcp_project"));

    let migrated = fs::read_to_string(config_path).unwrap();
    assert!(migrated.contains("infisical_project_id = \"test-project\""));
    assert!(!migrated.contains("gcp_project"));
}

#[test]
fn load_and_exec_read_real_age_encrypted_cache_without_network() {
    let home = TestHome::new();
    init_file_identity(&home);
    seed_cache(&home, "global", "A=one\nB=\"two words\"\n");

    home.rage()
        .args(["load", "global", "--format", "export", "--no-shell-hook"])
        .assert()
        .success()
        .stdout("export A='one'\nexport B='two words'\n");

    home.rage()
        .args(["exec", "global", "--", "/usr/bin/env"])
        .assert()
        .success()
        .stdout(predicates::str::contains("A=one\n"))
        .stdout(predicates::str::contains("B=two words\n"));
}

#[test]
fn keychain_identity_is_blocked_over_ssh_without_explicit_flag() {
    let home = TestHome::new();
    let identity_path = init_file_identity(&home);
    let recipient = config_value(&home, "age_recipient");
    seed_cache(&home, "global", "A=one\n");
    init_keychain_identity(&home, &recipient);
    let fake = fake_security_bin(&identity_path);

    home.rage()
        .env("PATH", prepend_path(&fake.bin_dir))
        .env("SSH_CONNECTION", "127.0.0.1 1 127.0.0.1 2")
        .args(["load", "global"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "refusing to read macOS Keychain identity from an SSH session",
        ));

    home.rage()
        .env("PATH", prepend_path(&fake.bin_dir))
        .env("SSH_CONNECTION", "127.0.0.1 1 127.0.0.1 2")
        .args(["load", "--allow-ssh-keychain", "--no-shell-hook", "global"])
        .assert()
        .success()
        .stdout("export A='one'\n");
}

#[test]
fn tui_refuses_keychain_identity_over_ssh() {
    let home = TestHome::new();
    let identity_path = init_file_identity(&home);
    let recipient = config_value(&home, "age_recipient");
    init_keychain_identity(&home, &recipient);
    let fake = fake_security_bin(&identity_path);

    home.rage()
        .env("PATH", prepend_path(&fake.bin_dir))
        .env("SSH_CONNECTION", "127.0.0.1 1 127.0.0.1 2")
        .arg("tui")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "refusing to read macOS Keychain identity from an SSH session",
        ));
}

#[test]
fn tui_refuses_to_open_when_stdout_is_not_a_tty() {
    let home = TestHome::new();
    init_file_identity(&home);

    home.rage()
        .arg("tui")
        .assert()
        .failure()
        .stderr(predicates::str::contains("`rage tui` requires a terminal"));
}

#[test]
fn keychain_init_requires_service_name() {
    let home = TestHome::new();

    home.rage()
        .args([
            "init",
            "--infisical-project-id",
            "test-project",
            "--age-recipient",
            "age1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq3zyg3z",
            "--age-identity-source",
            "keychain",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "--keychain-service is required when --age-identity-source keychain is used",
        ));
}

#[test]
fn auth_status_reports_infisical_token_env() {
    let home = TestHome::new();
    init_file_identity(&home);

    home.rage()
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout("auth: not-configured\n");

    home.rage()
        .env("INFISICAL_TOKEN", "fake-token")
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout("auth: infisical-token-env\n");

    let fake = FakeInfisical::new();
    fake.apply_machine_identity(&mut home.rage())
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout("auth: infisical-token-env\n");
}

#[test]
fn set_sync_get_and_list_round_trip_through_fake_infisical() {
    let home = TestHome::new();
    init_file_identity(&home);
    let fake = FakeInfisical::new();

    fake.apply(&mut home.rage())
        .args(["set", "project/foo/dev", "DATABASE_URL", "postgres://db"])
        .assert()
        .success()
        .stdout("updated project/foo/dev\n");

    fs::remove_file(home.cache_dir.join("rage-cHJvamVjdC9mb28vZGV2.env.age")).unwrap();

    fake.apply(&mut home.rage())
        .args(["sync", "project/foo/dev"])
        .assert()
        .success()
        .stdout("synced project/foo/dev\n");

    home.rage()
        .args(["get", "project/foo/dev", "DATABASE_URL"])
        .assert()
        .success()
        .stdout("postgres://db\n");

    fake.apply(&mut home.rage())
        .arg("list")
        .assert()
        .success()
        .stdout("project/foo/dev\n");
}

#[test]
fn list_shows_agents_bundle_when_agent_auth_is_imported() {
    let home = TestHome::new();
    init_file_identity(&home);
    let fake = FakeInfisical::new();
    fake.insert("/agents", "AUTHLESS_GROK_JSON", "agent-auth-json");

    fake.apply(&mut home.rage())
        .arg("list")
        .assert()
        .success()
        .stdout("agents\n");
}

#[test]
fn load_sync_fetches_missing_cache_and_output_formats_are_stable() {
    let home = TestHome::new();
    init_file_identity(&home);
    let fake = FakeInfisical::new();
    fake.insert("/", "A", "one");
    fake.insert("/", "B", "two words");

    fake.apply(&mut home.rage())
        .args(["load", "--sync", "global", "--format", "dotenv"])
        .assert()
        .success()
        .stdout("A=one\nB=\"two words\"\n");

    home.rage()
        .args(["load", "global", "--format", "json"])
        .assert()
        .success()
        .stdout("{\n  \"A\": \"one\",\n  \"B\": \"two words\"\n}\n");
}

#[test]
fn unset_removes_key_remotely_and_from_local_cache() {
    let home = TestHome::new();
    init_file_identity(&home);
    let fake = FakeInfisical::new();
    fake.insert("/", "A", "one");
    fake.insert("/", "B", "two");
    fake.insert("/", "AUTHLESS_GROK_JSON", "agent-auth-json");

    fake.apply(&mut home.rage())
        .args(["unset", "global", "A"])
        .assert()
        .success()
        .stdout("updated global\n");

    assert_eq!(fake.get("/", "A"), None);
    assert_eq!(fake.get("/", "B").unwrap(), "two");
    assert_eq!(
        fake.get("/", "AUTHLESS_GROK_JSON").unwrap(),
        "agent-auth-json"
    );

    home.rage()
        .args(["get", "global"])
        .assert()
        .success()
        .stdout("B=two\n");
}

#[test]
fn bundle_commands_reject_reserved_agent_auth_keys() {
    let home = TestHome::new();
    init_file_identity(&home);
    let fake = FakeInfisical::new();

    fake.apply(&mut home.rage())
        .args(["set", "global", "AUTHLESS_GROK_JSON", "value"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "environment key name is reserved for rage agent auth",
        ));
}

#[test]
fn sourced_load_hook_unsets_key_from_current_shell() {
    let home = TestHome::new();
    init_file_identity(&home);
    seed_cache(&home, "global", "A=one\nB=two\n");
    let fake = FakeInfisical::new();
    fake.insert("/", "A", "one");
    fake.insert("/", "B", "two");
    let rage_bin = assert_cmd::cargo::cargo_bin("rage");

    assert_cmd::Command::new("/bin/sh")
        .env("RAGE_CONFIG_DIR", &home.config_dir)
        .env("RAGE_CACHE_DIR", &home.cache_dir)
        .env("RAGE_INFISICAL_ENDPOINT", &fake.endpoint)
        .env("INFISICAL_TOKEN", "fake-token")
        .arg("-c")
        .arg(format!(
            r#"set -eu
eval "$("{}" load global)"
[ "$A" = one ]
rage unset global A
[ -z "${{A+x}}" ]
[ "$B" = two ]
"#,
            rage_bin.display()
        ))
        .assert()
        .success()
        .stdout(predicates::str::contains("updated global\n"));

    assert_eq!(fake.get("/", "A"), None);
    assert_eq!(fake.get("/", "B").unwrap(), "two");

    home.rage()
        .args(["get", "global"])
        .assert()
        .success()
        .stdout("B=two\n");
}

#[test]
fn ssh_sends_exports_over_stdin_without_putting_secret_in_arguments() {
    let home = TestHome::new();
    init_file_identity(&home);
    seed_cache(&home, "global", "TOKEN=\"a b'c\"\n");
    let fake = fake_ssh_bin();

    home.rage()
        .env("PATH", prepend_path(&fake.bin_dir))
        .env("RAGE_FAKE_SSH_SCRIPT", fake.script_file())
        .env("RAGE_FAKE_SSH_ARGS", fake.args_file())
        .args(["ssh", "host.example", "global", "--", "printenv", "TOKEN"])
        .assert()
        .success();

    let args = fs::read_to_string(fake.args_file()).unwrap();
    assert!(args.contains("host.example"));
    assert!(!args.contains("a b"));

    let script = fs::read_to_string(fake.script_file()).unwrap();
    assert!(script.contains("export TOKEN='a b'\\''c'"));
    assert!(script.contains("exec 'printenv' 'TOKEN'"));
}

#[test]
fn import_grok_writes_agent_auth_to_infisical() {
    let home = TestHome::new();
    init_file_identity(&home);
    let fake = FakeInfisical::new();
    let auth_path = home._tmp.path().join("grok-auth.json");
    fs::write(
        &auth_path,
        format!(
            r#"{{"{}":{{"key":"access-imported","refresh_token":"refresh-imported","expires_at":"2099-01-01T00:00:00.000Z"}}}}"#,
            grok_cached_auth_key()
        ),
    )
    .unwrap();

    fake.apply(&mut home.rage())
        .args(["import", "grok", auth_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout("imported grok auth\n");

    let raw = fake.get("/agents", "AUTHLESS_GROK_JSON").unwrap();
    assert!(raw.contains("access-imported"));
    fake.apply(&mut home.rage())
        .arg("list")
        .assert()
        .success()
        .stdout("agents\n");
    assert!(
        !home
            .config_dir
            .join("rage/agent-auth/grok.json.age")
            .exists()
    );
}

#[test]
fn rage_grok_migrates_legacy_root_agent_auth_to_agents_bundle() {
    let home = TestHome::new();
    init_file_identity(&home);
    let infisical = FakeInfisical::new();
    let auth = serde_json::json!({
        "client_id": "b1a00492-073a-47ea-816f-4c329264a828",
        "issuer": "https://auth.x.ai",
        "access_token": "access-old",
        "refresh_token": "refresh-old",
        "expires_at": "2099-01-01T00:00:00.000Z",
        "token_type": "Bearer",
    });
    infisical.insert("/", "AUTHLESS_GROK_JSON", &auth.to_string());
    let fake = fake_agent_bin(
        "grok",
        "#!/bin/sh\nprintf 'token=%s\\n' \"$GROK_CODE_XAI_API_KEY\"\n",
    );

    infisical
        .apply(&mut home.rage())
        .arg("list")
        .assert()
        .success()
        .stdout("agents\n");

    infisical
        .apply(&mut home.rage())
        .args(["sync", "agents"])
        .assert()
        .success()
        .stdout("synced agents\n");

    infisical
        .apply(&mut home.rage())
        .env("PATH", prepend_path(&fake.bin_dir))
        .arg("grok")
        .assert()
        .success()
        .stdout("token=access-old\n");

    assert_eq!(
        infisical.get("/agents", "AUTHLESS_GROK_JSON").unwrap(),
        auth.to_string()
    );
}

#[test]
fn rage_grok_injects_access_token_only_into_child_env() {
    let home = TestHome::new();
    init_file_identity(&home);
    let infisical = FakeInfisical::new();
    import_grok_auth(
        &home,
        &infisical,
        "access-old",
        "refresh-old",
        "2099-01-01T00:00:00.000Z",
    );
    let fake = fake_agent_bin(
        "grok",
        "#!/bin/sh\nprintf 'token=%s\\nargs=%s\\nrefresh=%s\\n' \"$GROK_CODE_XAI_API_KEY\" \"$*\" \"${refresh_token-unset}\"\n",
    );

    infisical
        .apply(&mut home.rage())
        .env("PATH", prepend_path(&fake.bin_dir))
        .args(["grok", "--", "-p", "hello"])
        .assert()
        .success()
        .stdout("token=access-old\nargs=-p hello\nrefresh=unset\n");
}

#[test]
fn rage_grok_refreshes_expired_auth_and_persists_rotation() {
    let home = TestHome::new();
    init_file_identity(&home);
    let infisical = FakeInfisical::new();
    import_grok_auth(
        &home,
        &infisical,
        "access-old",
        "refresh-old",
        "2000-01-01T00:00:00.000Z",
    );
    let oauth = FakeAgentOAuth::new();
    let fake = fake_agent_bin(
        "grok",
        "#!/bin/sh\nprintf 'token=%s\\n' \"$GROK_CODE_XAI_API_KEY\"\n",
    );

    infisical
        .apply(&mut home.rage())
        .env("PATH", prepend_path(&fake.bin_dir))
        .env("GROK_AUTH_ENDPOINT_URL", oauth.grok_endpoint())
        .arg("grok")
        .assert()
        .success()
        .stdout("token=access-new\n");

    assert_eq!(oauth.grok_calls(), 1);

    infisical
        .apply(&mut home.rage())
        .env("PATH", prepend_path(&fake.bin_dir))
        .env("GROK_AUTH_ENDPOINT_URL", oauth.grok_endpoint())
        .arg("grok")
        .assert()
        .success()
        .stdout("token=access-new\n");

    assert_eq!(oauth.grok_calls(), 1);
}

#[test]
fn rage_codex_writes_managed_auth_json_and_cleans_up_created_file() {
    let home = TestHome::new();
    init_file_identity(&home);
    let infisical = FakeInfisical::new();
    import_codex_auth(
        &home,
        &infisical,
        &future_jwt(),
        "id-token",
        "refresh-old",
        None,
    );
    let codex_home = home._tmp.path().join("codex-home");
    let marker = home._tmp.path().join("codex-marker");
    let fake = fake_agent_bin(
        "codex",
        "#!/bin/sh\ntest -f \"$CODEX_HOME/auth.json\"\ngrep -q '\"auth_mode\": \"chatgpt\"' \"$CODEX_HOME/auth.json\"\nprintf '%s' \"$CODEX_HOME\" > \"$TEST_CODEX_MARKER\"\n",
    );

    infisical
        .apply(&mut home.rage())
        .env("PATH", prepend_path(&fake.bin_dir))
        .env("CODEX_HOME", &codex_home)
        .env("TEST_CODEX_MARKER", &marker)
        .args(["codex", "run", "hello"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(marker).unwrap(),
        codex_home.display().to_string()
    );
    assert!(!codex_home.join("auth.json").exists());
}

#[test]
fn rage_codex_refreshes_expired_auth_and_persists_rotation() {
    let home = TestHome::new();
    init_file_identity(&home);
    let infisical = FakeInfisical::new();
    import_codex_auth(
        &home,
        &infisical,
        &jwt_with_exp(946_684_800),
        "id-token-old",
        "refresh-old",
        None,
    );
    let oauth = FakeAgentOAuth::new();
    let codex_home = home._tmp.path().join("codex-refresh-home");
    let fake = fake_agent_bin(
        "codex",
        "#!/bin/sh\ngrep -q 'refresh-new' \"$CODEX_HOME/auth.json\"\ngrep -q 'id-token-new' \"$CODEX_HOME/auth.json\"\n",
    );

    infisical
        .apply(&mut home.rage())
        .env("PATH", prepend_path(&fake.bin_dir))
        .env("CODEX_HOME", &codex_home)
        .env("CODEX_AUTH_ENDPOINT_URL", oauth.codex_endpoint())
        .arg("codex")
        .assert()
        .success();

    assert_eq!(oauth.codex_calls(), 1);

    infisical
        .apply(&mut home.rage())
        .env("PATH", prepend_path(&fake.bin_dir))
        .env("CODEX_HOME", &codex_home)
        .env("CODEX_AUTH_ENDPOINT_URL", oauth.codex_endpoint())
        .arg("codex")
        .assert()
        .success();

    assert_eq!(oauth.codex_calls(), 1);
}

fn init_file_identity(home: &TestHome) -> PathBuf {
    let identity_path = home.config_dir.join("rage/key.txt");
    home.rage()
        .args([
            "init",
            "--infisical-project-id",
            "test-project",
            "--age-identity",
            identity_path.to_str().unwrap(),
        ])
        .assert()
        .success();
    identity_path
}

fn init_keychain_identity(home: &TestHome, recipient: &str) {
    home.rage()
        .args([
            "init",
            "--infisical-project-id",
            "test-project",
            "--age-recipient",
            recipient,
            "--age-identity",
            "acct",
            "--age-identity-source",
            "keychain",
            "--keychain-service",
            "rage-test",
            "--keychain-account",
            "acct",
        ])
        .assert()
        .success();
}

fn seed_cache(home: &TestHome, bundle: &str, payload: &str) {
    let fake = FakeInfisical::new();
    let secret_path = if bundle == "global" {
        "/".to_string()
    } else {
        format!("/{}", bundle.trim_matches('/'))
    };
    for line in payload.lines() {
        let (key, value) = line.split_once('=').unwrap();
        fake.insert(&secret_path, key, value.trim_matches('"'));
    }
    fake.apply(&mut home.rage())
        .args(["sync", bundle])
        .assert()
        .success();
}

fn config_value(home: &TestHome, key: &str) -> String {
    let config = fs::read_to_string(home.config_dir.join("rage/config.toml")).unwrap();
    config
        .lines()
        .find_map(|line| {
            let (k, v) = line.split_once(" = ")?;
            (k == key).then(|| v.trim_matches('"').to_string())
        })
        .unwrap_or_else(|| panic!("missing config key {key}"))
}

fn import_grok_auth(
    home: &TestHome,
    infisical: &FakeInfisical,
    access: &str,
    refresh: &str,
    expires_at: &str,
) {
    let auth_path = home._tmp.path().join(format!("grok-auth-{access}.json"));
    fs::write(
        &auth_path,
        format!(
            r#"{{"{}":{{"key":"{}","refresh_token":"{}","expires_at":"{}"}}}}"#,
            grok_cached_auth_key(),
            access,
            refresh,
            expires_at
        ),
    )
    .unwrap();
    infisical
        .apply(&mut home.rage())
        .args(["import", "grok", auth_path.to_str().unwrap()])
        .assert()
        .success();
}

fn import_codex_auth(
    home: &TestHome,
    infisical: &FakeInfisical,
    access_token: &str,
    id_token: &str,
    refresh_token: &str,
    account_id: Option<&str>,
) {
    let auth_path = home._tmp.path().join("codex-auth.json");
    let mut tokens = serde_json::json!({
        "id_token": id_token,
        "access_token": access_token,
        "refresh_token": refresh_token,
    });
    if let Some(account_id) = account_id {
        tokens["account_id"] = serde_json::json!(account_id);
    }
    fs::write(
        &auth_path,
        serde_json::json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": serde_json::Value::Null,
            "tokens": tokens,
            "last_refresh": "2026-05-16T12:34:56.000Z"
        })
        .to_string(),
    )
    .unwrap();
    infisical
        .apply(&mut home.rage())
        .args(["import", "codex", auth_path.to_str().unwrap()])
        .assert()
        .success();
}

fn grok_cached_auth_key() -> String {
    "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828".to_string()
}

fn future_jwt() -> String {
    let exp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;
    jwt_with_exp(exp)
}

fn jwt_with_exp(exp: u64) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#));
    format!("{header}.{payload}.")
}

fn prepend_path(dir: &Path) -> String {
    let original = std::env::var("PATH").unwrap_or_default();
    format!("{}:{original}", dir.display())
}

struct FakeAgentBin {
    _tmp: TempDir,
    bin_dir: PathBuf,
}

fn fake_agent_bin(name: &str, script: &str) -> FakeAgentBin {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let bin = bin_dir.join(name);
    fs::write(&bin, script).unwrap();
    make_executable(&bin);
    FakeAgentBin { _tmp: tmp, bin_dir }
}

struct FakeAgentOAuth {
    endpoint: String,
    state: Arc<Mutex<FakeAgentOAuthState>>,
}

#[derive(Default)]
struct FakeAgentOAuthState {
    grok_calls: usize,
    codex_calls: usize,
}

impl FakeAgentOAuth {
    fn new() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.server_addr());
        let state = Arc::new(Mutex::new(FakeAgentOAuthState::default()));
        let thread_state = state.clone();
        thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let method = request.method().as_str().to_string();
                let path = request.url().split('?').next().unwrap_or("").to_string();
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let response = if method == "POST" && path == "/grok" {
                    assert!(body.contains("grant_type=refresh_token"));
                    assert!(body.contains("refresh_token=refresh-old"));
                    assert!(body.contains("client_id=b1a00492-073a-47ea-816f-4c329264a828"));
                    thread_state.lock().unwrap().grok_calls += 1;
                    json_response(
                        200,
                        serde_json::json!({
                            "access_token": "access-new",
                            "refresh_token": "refresh-new",
                            "expires_in": 3600,
                            "scope": "openid profile email offline_access grok-cli:access api:access",
                            "token_type": "Bearer"
                        }),
                    )
                } else if method == "POST" && path == "/codex" {
                    assert!(body.contains("\"grant_type\":\"refresh_token\""));
                    assert!(body.contains("\"refresh_token\":\"refresh-old\""));
                    assert!(body.contains("\"client_id\":\"app_EMoamEEZ73f0CkXaXp7hrann\""));
                    thread_state.lock().unwrap().codex_calls += 1;
                    json_response(
                        200,
                        serde_json::json!({
                            "access_token": future_jwt(),
                            "refresh_token": "refresh-new",
                            "id_token": "id-token-new"
                        }),
                    )
                } else {
                    json_response(404, serde_json::json!({ "error": "not found" }))
                };
                request.respond(response).unwrap();
            }
        });
        Self { endpoint, state }
    }

    fn grok_endpoint(&self) -> String {
        format!("{}/grok", self.endpoint)
    }

    fn grok_calls(&self) -> usize {
        self.state.lock().unwrap().grok_calls
    }

    fn codex_endpoint(&self) -> String {
        format!("{}/codex", self.endpoint)
    }

    fn codex_calls(&self) -> usize {
        self.state.lock().unwrap().codex_calls
    }
}

struct FakeSecurity {
    _tmp: TempDir,
    bin_dir: PathBuf,
}

fn fake_security_bin(identity_path: &Path) -> FakeSecurity {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("security");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = find-generic-password ]; then cat '{}'; exit 0; fi\nexit 64\n",
            identity_path.display()
        ),
    )
    .unwrap();
    make_executable(&script);
    FakeSecurity { _tmp: tmp, bin_dir }
}

struct FakeInfisical {
    endpoint: String,
    store: Arc<Mutex<BTreeMap<(String, String), String>>>,
}

impl FakeInfisical {
    fn new() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.server_addr());
        let store = Arc::new(Mutex::new(BTreeMap::<(String, String), String>::new()));
        let thread_store = store.clone();
        thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let method = request.method().as_str().to_string();
                let url = request.url().to_string();
                let path = url.split('?').next().unwrap_or(&url).to_string();
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let response = handle_infisical_request(&thread_store, &method, &path, &url, &body);
                request.respond(response).unwrap();
            }
        });
        Self { endpoint, store }
    }

    fn apply<'a>(&self, cmd: &'a mut assert_cmd::Command) -> &'a mut assert_cmd::Command {
        cmd.env("RAGE_INFISICAL_ENDPOINT", &self.endpoint)
            .env("INFISICAL_TOKEN", "fake-token")
    }

    fn apply_machine_identity<'a>(
        &self,
        cmd: &'a mut assert_cmd::Command,
    ) -> &'a mut assert_cmd::Command {
        cmd.env("RAGE_INFISICAL_ENDPOINT", &self.endpoint)
            .env("INFISICAL_MACHINE_IDENTITY_CLIENT_ID", "client-id")
            .env("INFISICAL_MACHINE_IDENTITY_CLIENT_SECRET", "client-secret")
    }

    fn insert(&self, secret_path: &str, key: &str, value: &str) {
        self.store.lock().unwrap().insert(
            (normalize_test_path(secret_path), key.to_string()),
            value.to_string(),
        );
    }

    fn get(&self, secret_path: &str, key: &str) -> Option<String> {
        self.store
            .lock()
            .unwrap()
            .get(&(normalize_test_path(secret_path), key.to_string()))
            .cloned()
    }
}

fn handle_infisical_request(
    store: &Arc<Mutex<BTreeMap<(String, String), String>>>,
    method: &str,
    path: &str,
    url: &str,
    body: &str,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    if method == "GET" && path == "/api/v2/service-token" {
        return json_response(
            200,
            serde_json::json!({
                "serviceToken": {
                    "projectId": "test-project",
                    "scopes": [{ "secretPath": "/", "environment": "prod" }]
                }
            }),
        );
    }

    if method == "POST" && path == "/api/v1/auth/universal-auth/login" {
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(value["clientId"], "client-id");
        assert_eq!(value["clientSecret"], "client-secret");
        return json_response(
            200,
            serde_json::json!({
                "accessToken": "machine-token",
                "expiresIn": 3600,
                "accessTokenMaxTTL": 3600,
                "tokenType": "Bearer"
            }),
        );
    }

    if method == "GET" && path == "/api/v1/projects" {
        return json_response(
            200,
            serde_json::json!({
                "projects": [{
                    "id": "test-project",
                    "name": "authless",
                    "slug": "authless"
                }]
            }),
        );
    }

    if method == "POST" && path == "/api/v2/folders" {
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        return json_response(
            200,
            serde_json::json!({
                "folder": {
                    "name": value["name"].as_str().unwrap_or(""),
                    "path": value["path"].as_str().unwrap_or("/")
                }
            }),
        );
    }

    if method == "GET" && path == "/api/v4/secrets" {
        let query = parse_query(url);
        let secret_path = normalize_test_path(query.get("secretPath").map_or("/", String::as_str));
        let recursive = query.get("recursive").is_some_and(|value| value == "true");
        let view = query
            .get("viewSecretValue")
            .is_none_or(|value| value == "true");
        let secrets: Vec<_> = store
            .lock()
            .unwrap()
            .iter()
            .filter(|((path, _), _)| {
                if recursive {
                    path == &secret_path
                        || secret_path == "/"
                        || path.starts_with(&format!("{secret_path}/"))
                } else {
                    path == &secret_path
                }
            })
            .map(|((path, key), value)| {
                serde_json::json!({
                    "secretKey": key,
                    "secretValue": if view { value.as_str() } else { "" },
                    "secretPath": path
                })
            })
            .collect();
        return json_response(200, serde_json::json!({ "secrets": secrets }));
    }

    let Some(encoded_key) = path.strip_prefix("/api/v4/secrets/") else {
        return json_response(404, serde_json::json!({ "error": "not found" }));
    };
    let key = url_decode(encoded_key);

    if method == "GET" {
        let query = parse_query(url);
        let secret_path = normalize_test_path(query.get("secretPath").map_or("/", String::as_str));
        if let Some(value) = store
            .lock()
            .unwrap()
            .get(&(secret_path.clone(), key.clone()))
            .cloned()
        {
            return json_response(
                200,
                serde_json::json!({
                    "secret": { "secretKey": key, "secretValue": value, "secretPath": secret_path }
                }),
            );
        }
        return json_response(404, serde_json::json!({ "error": "not found" }));
    }

    if method == "POST" || method == "PATCH" {
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        let secret_path = normalize_test_path(value["secretPath"].as_str().unwrap_or("/"));
        let secret_value = value["secretValue"].as_str().unwrap_or("").to_string();
        store
            .lock()
            .unwrap()
            .insert((secret_path.clone(), key.clone()), secret_value.clone());
        return json_response(
            200,
            serde_json::json!({
                "secret": { "secretKey": key, "secretValue": secret_value, "secretPath": secret_path }
            }),
        );
    }

    if method == "DELETE" {
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        let secret_path = normalize_test_path(value["secretPath"].as_str().unwrap_or("/"));
        store.lock().unwrap().remove(&(secret_path, key));
        return json_response(200, serde_json::json!({}));
    }

    json_response(404, serde_json::json!({ "error": "not found" }))
}

fn normalize_test_path(path: &str) -> String {
    if path == "/" || path.trim().is_empty() {
        "/".to_string()
    } else {
        format!("/{}", path.trim_matches('/'))
    }
}

fn parse_query(url: &str) -> BTreeMap<String, String> {
    url.split('?')
        .nth(1)
        .unwrap_or("")
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((url_decode(key), url_decode(value)))
        })
        .collect()
}

fn url_decode(value: &str) -> String {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(hex) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                    out.push(hex);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap()
}

fn json_response(
    status: u16,
    value: serde_json::Value,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    tiny_http::Response::from_string(value.to_string()).with_status_code(status)
}

struct FakeSsh {
    _tmp: TempDir,
    bin_dir: PathBuf,
}

impl FakeSsh {
    fn script_file(&self) -> PathBuf {
        self._tmp.path().join("script")
    }

    fn args_file(&self) -> PathBuf {
        self._tmp.path().join("args")
    }
}

fn fake_ssh_bin() -> FakeSsh {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("ssh");
    fs::write(
        &script,
        r#"#!/bin/sh
printf '%s\n' "$@" > "${RAGE_FAKE_SSH_ARGS:?}"
cat > "${RAGE_FAKE_SSH_SCRIPT:?}"
exit 0
"#,
    )
    .unwrap();
    make_executable(&script);
    FakeSsh { _tmp: tmp, bin_dir }
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }
}
