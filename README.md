# OKF CLI

`okf` is the command-line interface for Markdown-based Open Knowledge Format bundles. It is a thin, deterministic adapter over the `okf` Rust SDK.

## Commands

```text
okf init
okf validate
okf list
okf get <id-or-alias>
okf inspect <id-or-alias>
okf search <query>
okf graph
```

Use `--bundle <directory>` to select a bundle and `--output json` for a stable machine-readable envelope.

```bash
okf --bundle ./knowledge --output json search "runtime architecture" --tag architecture
```

## Exit codes

- `0`: command completed and validation policy passed.
- `1`: validation diagnostics failed the selected policy.
- `2`: command-line usage error.
- `3`: operational or output error.

## SDK dependency

Until the SDK is published to crates.io, `vendor/okf` contains a source snapshot derived from `JarynXu/okf-sdk` commit `483f192e1e9197bb3f18f7809d51aeb9b4868ea1`. CLI command handlers call that dependency rather than reimplementing parsing, validation, graph traversal, or retrieval.

## Status

The `0.1.0-alpha.1` API and JSON schema may evolve before the first stable release.
