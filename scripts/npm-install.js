#!/usr/bin/env node

const crypto = require("node:crypto");
const fs = require("node:fs");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const repo = process.env.RAGE_REPO || "thehumanworks/rage";
const packageRoot = path.resolve(__dirname, "..");
const packageJson = require(path.join(packageRoot, "package.json"));
const requestedVersion =
  process.env.RAGE_NPM_VERSION || process.env.RAGE_VERSION || `v${packageJson.version}`;
const vendorDir = path.join(packageRoot, "vendor");
const token = process.env.RAGE_GITHUB_TOKEN || process.env.GITHUB_TOKEN || ghToken();

main().catch((error) => {
  console.error(`rage npm install: ERROR: ${error.message}`);
  process.exit(1);
});

async function main() {
  if (process.env.RAGE_NPM_SKIP_DOWNLOAD === "1") {
    log("skipping binary download because RAGE_NPM_SKIP_DOWNLOAD=1");
    return;
  }

  const target = detectTarget(process.platform, process.arch);
  const release = await getRelease(requestedVersion);
  const tag = release.tag_name;
  if (!tag) {
    throw new Error("GitHub release response did not include tag_name");
  }
  const version = tag.startsWith("v") ? tag.slice(1) : tag;
  const extension = target.endsWith("windows-msvc") ? "zip" : "tar.gz";
  const archiveName = `rage-${version}-${target}.${extension}`;
  const archiveAsset = findAsset(release, archiveName);
  const sumsAsset = findAsset(release, "SHA256SUMS");

  log(`target ${target}`);
  log(`release ${tag}`);
  log(`archive ${archiveName}`);
  log(`auth ${token ? "yes" : "no"}`);

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "rage-npm-"));
  try {
    const archivePath = path.join(tmpDir, archiveName);
    const sumsPath = path.join(tmpDir, "SHA256SUMS");
    await downloadAsset(archiveAsset.url, archivePath);
    await downloadAsset(sumsAsset.url, sumsPath);
    verifyChecksum(archivePath, archiveName, sumsPath);
    extractArchive(archivePath, tmpDir, extension);
    installBinary(tmpDir, version, target);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

function detectTarget(platform, arch) {
  if (platform === "darwin" && arch === "arm64") {
    return "aarch64-apple-darwin";
  }
  if (platform === "darwin" && arch === "x64") {
    throw new Error(
      "macOS Intel (x64) is not currently in the rage release matrix; build from source with `cargo install --path .`",
    );
  }
  if (platform === "linux" && arch === "x64") {
    return "x86_64-unknown-linux-gnu";
  }
  if (platform === "linux" && arch === "arm64") {
    return "aarch64-unknown-linux-gnu";
  }
  if (platform === "win32" && arch === "x64") {
    return "x86_64-pc-windows-msvc";
  }
  throw new Error(`unsupported platform: ${platform}/${arch}`);
}

async function getRelease(version) {
  const encodedRepo = repo
    .split("/")
    .map((part) => encodeURIComponent(part))
    .join("/");
  const url =
    version === "latest"
      ? `https://api.github.com/repos/${encodedRepo}/releases/latest`
      : `https://api.github.com/repos/${encodedRepo}/releases/tags/${encodeURIComponent(version)}`;
  const body = await requestBuffer(url, {
    Accept: "application/vnd.github+json",
  });
  return JSON.parse(body.toString("utf8"));
}

function findAsset(release, name) {
  const asset = (release.assets || []).find((candidate) => candidate.name === name);
  if (!asset) {
    throw new Error(`release ${release.tag_name || ""} does not contain asset ${name}`);
  }
  return asset;
}

async function downloadAsset(url, destination) {
  const body = await requestBuffer(url, {
    Accept: "application/octet-stream",
  });
  fs.writeFileSync(destination, body);
}

function requestBuffer(url, headers, redirectCount = 0) {
  if (redirectCount > 5) {
    return Promise.reject(new Error(`too many redirects while fetching ${url}`));
  }
  return new Promise((resolve, reject) => {
    const requestHeaders = {
      "User-Agent": "@nothumanwork/rage npm installer",
      ...headers,
    };
    if (token) {
      requestHeaders.Authorization = `Bearer ${token}`;
    }
    const request = https.get(url, { headers: requestHeaders }, (response) => {
      const location = response.headers.location;
      if (
        location &&
        response.statusCode >= 300 &&
        response.statusCode < 400
      ) {
        response.resume();
        resolve(requestBuffer(new URL(location, url).toString(), headers, redirectCount + 1));
        return;
      }
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => {
        const body = Buffer.concat(chunks);
        if (response.statusCode < 200 || response.statusCode >= 300) {
          reject(
            new Error(
              `HTTP ${response.statusCode} fetching ${url}: ${body.toString("utf8").slice(0, 500)}`,
            ),
          );
          return;
        }
        resolve(body);
      });
    });
    request.on("error", reject);
  });
}

function verifyChecksum(archivePath, archiveName, sumsPath) {
  const sums = fs.readFileSync(sumsPath, "utf8");
  const expected = sums
    .split(/\r?\n/)
    .map((line) => line.trim().split(/\s+/))
    .find((parts) => parts[1] === archiveName || parts[1] === `*${archiveName}`)?.[0];
  if (!expected) {
    throw new Error(`checksum for ${archiveName} was not found in SHA256SUMS`);
  }
  const actual = crypto
    .createHash("sha256")
    .update(fs.readFileSync(archivePath))
    .digest("hex");
  if (actual !== expected) {
    throw new Error(`checksum mismatch for ${archiveName}`);
  }
  log("checksum ok");
}

function extractArchive(archivePath, tmpDir, extension) {
  const result =
    extension === "zip"
      ? spawnSync(
          "powershell.exe",
          [
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "Expand-Archive -LiteralPath $args[0] -DestinationPath $args[1] -Force",
            archivePath,
            tmpDir,
          ],
          { stdio: "inherit" },
        )
      : spawnSync("tar", ["-xzf", archivePath, "-C", tmpDir], { stdio: "inherit" });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`archive extraction failed with exit code ${result.status}`);
  }
}

function installBinary(tmpDir, version, target) {
  const executable = target.endsWith("windows-msvc") ? "rage.exe" : "rage";
  const extracted = path.join(tmpDir, `rage-${version}-${target}`, executable);
  if (!fs.existsSync(extracted)) {
    throw new Error(`archive did not contain ${executable} at expected path`);
  }
  fs.mkdirSync(vendorDir, { recursive: true });
  const destination = path.join(vendorDir, executable);
  fs.copyFileSync(extracted, destination);
  if (process.platform !== "win32") {
    fs.chmodSync(destination, 0o755);
  }
  log(`installed ${destination}`);
}

function ghToken() {
  const result = spawnSync("gh", ["auth", "token"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  });
  if (result.status === 0) {
    return result.stdout.trim();
  }
  return "";
}

function log(message) {
  console.error(`rage npm install: ${message}`);
}
