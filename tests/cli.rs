use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

use base64::{Engine, engine::general_purpose::STANDARD};
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
            .env("RAGE_CACHE_DIR", &self.cache_dir);
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
            "--gcp-project",
            "test-project",
            "--age-identity",
            identity_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let config = fs::read_to_string(home.config_dir.join("rage/config.toml")).unwrap();
    assert!(config.contains("gcp_project = \"test-project\""));
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
fn keychain_init_requires_service_name() {
    let home = TestHome::new();

    home.rage()
        .args([
            "init",
            "--gcp-project",
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
fn auth_import_service_account_stores_encrypted_credential() {
    let home = TestHome::new();
    init_file_identity(&home);
    let json = r#"{
      "type": "service_account",
      "client_email": "rage-test@example.iam.gserviceaccount.com",
      "private_key": "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----\n",
      "private_key_id": "key-id",
      "token_uri": "https://oauth2.googleapis.com/token"
    }"#;

    home.rage()
        .write_stdin(json)
        .args(["auth", "import-service-account"])
        .assert()
        .success()
        .stdout("imported service account rage-test@example.iam.gserviceaccount.com\n");

    let encrypted = home.config_dir.join("rage/gcp-service-account.json.age");
    assert!(encrypted.exists());
    let raw = fs::read(encrypted).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("private_key"));

    home.rage()
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout("auth: encrypted-service-account\n");
}

#[test]
fn set_sync_get_and_list_round_trip_through_fake_secret_manager() {
    let home = TestHome::new();
    init_file_identity(&home);
    let fake = FakeSecretManager::new();

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
fn load_sync_fetches_missing_cache_and_output_formats_are_stable() {
    let home = TestHome::new();
    init_file_identity(&home);
    let fake = FakeSecretManager::new();
    fake.insert("rage-Z2xvYmFs", "A=one\nB=\"two words\"\n");

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
    let fake = FakeSecretManager::new();
    fake.insert("rage-Z2xvYmFs", "A=one\nB=two\n");

    fake.apply(&mut home.rage())
        .args(["unset", "global", "A"])
        .assert()
        .success()
        .stdout("updated global\n");

    assert_eq!(fake.get("rage-Z2xvYmFs").unwrap(), "B=two\n");

    home.rage()
        .args(["get", "global"])
        .assert()
        .success()
        .stdout("B=two\n");
}

#[test]
fn sourced_load_hook_unsets_key_from_current_shell() {
    let home = TestHome::new();
    init_file_identity(&home);
    seed_cache(&home, "global", "A=one\nB=two\n");
    let fake = FakeSecretManager::new();
    fake.insert("rage-Z2xvYmFs", "A=one\nB=two\n");
    let rage_bin = assert_cmd::cargo::cargo_bin("rage");

    assert_cmd::Command::new("/bin/sh")
        .env("RAGE_CONFIG_DIR", &home.config_dir)
        .env("RAGE_CACHE_DIR", &home.cache_dir)
        .env("RAGE_GCP_SECRET_MANAGER_ENDPOINT", &fake.endpoint)
        .env("RAGE_GCP_ACCESS_TOKEN", "fake-token")
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

    assert_eq!(fake.get("rage-Z2xvYmFs").unwrap(), "B=two\n");

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

fn init_file_identity(home: &TestHome) -> PathBuf {
    let identity_path = home.config_dir.join("rage/key.txt");
    home.rage()
        .args([
            "init",
            "--gcp-project",
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
            "--gcp-project",
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
    let fake = FakeSecretManager::new();
    fake.insert(&format!("rage-{}", bundle_id(bundle)), payload);
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

fn bundle_id(bundle: &str) -> String {
    match bundle {
        "global" => "Z2xvYmFs".to_string(),
        "project/foo/dev" => "cHJvamVjdC9mb28vZGV2".to_string(),
        other => panic!("unknown test bundle: {other}"),
    }
}

fn prepend_path(dir: &Path) -> String {
    let original = std::env::var("PATH").unwrap_or_default();
    format!("{}:{original}", dir.display())
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

struct FakeSecretManager {
    endpoint: String,
    store: Arc<Mutex<BTreeMap<String, String>>>,
}

impl FakeSecretManager {
    fn new() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.server_addr());
        let store = Arc::new(Mutex::new(BTreeMap::<String, String>::new()));
        let thread_store = store.clone();
        thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let method = request.method().as_str().to_string();
                let url = request.url().to_string();
                let path = url.split('?').next().unwrap_or(&url).to_string();
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let response =
                    handle_secret_manager_request(&thread_store, &method, &path, &url, &body);
                request.respond(response).unwrap();
            }
        });
        Self { endpoint, store }
    }

    fn apply<'a>(&self, cmd: &'a mut assert_cmd::Command) -> &'a mut assert_cmd::Command {
        cmd.env("RAGE_GCP_SECRET_MANAGER_ENDPOINT", &self.endpoint)
            .env("RAGE_GCP_ACCESS_TOKEN", "fake-token")
    }

    fn insert(&self, secret_id: &str, payload: &str) {
        self.store
            .lock()
            .unwrap()
            .insert(secret_id.to_string(), payload.to_string());
    }

    fn get(&self, secret_id: &str) -> Option<String> {
        self.store.lock().unwrap().get(secret_id).cloned()
    }
}

fn handle_secret_manager_request(
    store: &Arc<Mutex<BTreeMap<String, String>>>,
    method: &str,
    path: &str,
    url: &str,
    body: &str,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let collection = "/v1/projects/test-project/secrets";
    if method == "GET" && path == collection {
        let secrets: Vec<_> = store
            .lock()
            .unwrap()
            .keys()
            .map(|id| serde_json::json!({ "name": format!("projects/test-project/secrets/{id}") }))
            .collect();
        return json_response(200, serde_json::json!({ "secrets": secrets }));
    }

    if method == "POST" && path == collection {
        let secret_id = url
            .split('?')
            .nth(1)
            .and_then(|query| {
                query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("secretId="))
            })
            .unwrap_or("");
        store
            .lock()
            .unwrap()
            .entry(secret_id.to_string())
            .or_default();
        return json_response(
            200,
            serde_json::json!({ "name": format!("projects/test-project/secrets/{secret_id}") }),
        );
    }

    let Some(secret_id) = path.strip_prefix(&(collection.to_string() + "/")) else {
        return json_response(404, serde_json::json!({ "error": "not found" }));
    };

    if method == "GET" && secret_id.ends_with("/versions/latest:access") {
        let id = secret_id.trim_end_matches("/versions/latest:access");
        if let Some(payload) = store.lock().unwrap().get(id).cloned() {
            return json_response(
                200,
                serde_json::json!({
                    "payload": { "data": STANDARD.encode(payload.as_bytes()) }
                }),
            );
        }
        return json_response(404, serde_json::json!({ "error": "not found" }));
    }

    if method == "POST" && secret_id.ends_with(":addVersion") {
        let id = secret_id.trim_end_matches(":addVersion");
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        let data = value["payload"]["data"].as_str().unwrap();
        let payload = String::from_utf8(STANDARD.decode(data).unwrap()).unwrap();
        store.lock().unwrap().insert(id.to_string(), payload);
        return json_response(
            200,
            serde_json::json!({ "name": format!("projects/test-project/secrets/{id}/versions/1") }),
        );
    }

    if method == "DELETE" {
        store.lock().unwrap().remove(secret_id);
        return json_response(200, serde_json::json!({}));
    }

    if method == "GET" {
        if store.lock().unwrap().contains_key(secret_id) {
            return json_response(
                200,
                serde_json::json!({ "name": format!("projects/test-project/secrets/{secret_id}") }),
            );
        }
        return json_response(404, serde_json::json!({ "error": "not found" }));
    }

    json_response(404, serde_json::json!({ "error": "not found" }))
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
