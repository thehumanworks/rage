#!/usr/bin/env node

const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const executable = process.platform === "win32" ? "rage.exe" : "rage";
const binary = path.join(__dirname, "..", "vendor", executable);

if (!fs.existsSync(binary)) {
  const installer = path.join(__dirname, "..", "scripts", "npm-install.js");
  const install = spawn(process.execPath, [installer], {
    stdio: "inherit",
    windowsHide: false,
  });
  install.on("error", (error) => {
    console.error(`failed to install rage binary: ${error.message}`);
    process.exit(1);
  });
  install.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    if (code !== 0) {
      process.exit(code ?? 1);
      return;
    }
    run();
  });
} else {
  run();
}

function run() {
  const child = spawn(binary, process.argv.slice(2), {
    stdio: "inherit",
    windowsHide: false,
  });

  child.on("error", (error) => {
    if (error.code === "ENOENT") {
      console.error(
        "rage binary is not installed. Reinstall @nothumanwork/rage or run npm rebuild @nothumanwork/rage.",
      );
    } else {
      console.error(error.message);
    }
    process.exit(1);
  });

  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 1);
  });
}
