#!/usr/bin/env node

const { spawn } = require("node:child_process");
const path = require("node:path");

const executable = process.platform === "win32" ? "rage.exe" : "rage";
const binary = path.join(__dirname, "..", "vendor", executable);

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
