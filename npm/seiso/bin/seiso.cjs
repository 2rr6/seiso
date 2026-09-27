#!/usr/bin/env node
"use strict";

const path = require("node:path");
const { spawnSync } = require("node:child_process");
const binaries = {
  "linux-x64": "linux-x64/seiso",
  "win32-x64": "win32-x64/seiso.exe",
};
const platform = `${process.platform}-${process.arch}`;
const binary = binaries[platform];
if (!binary) {
  console.error(`seiso: no bundled binary for ${platform}; install from source with cargo install seiso.`);
  process.exit(2);
}
const result = spawnSync(path.join(__dirname, "..", "native", binary), process.argv.slice(2), {
  stdio: "inherit",
});
if (result.error) {
  console.error(`seiso: cannot start the bundled binary: ${result.error.message}`);
  process.exit(2);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
} else {
  process.exit(result.status ?? 2);
}
