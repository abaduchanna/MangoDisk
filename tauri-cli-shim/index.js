// Bare-specifier shim: gradle BuildTask runs `node tauri ...` from repo root.
// Load the real @tauri-apps/cli from node_modules.
const path = require("path")
const cli = path.join(__dirname, "..", "node_modules", "@tauri-apps", "cli", "tauri.js")
process.argv.splice(1, 1, cli)
require(cli)
