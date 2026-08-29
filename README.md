# OKF CLI

`okf` is the command-line interface for Open Knowledge Format bundles and pluggable OKF Libraries. It remains a thin, deterministic adapter over the `okf` Rust SDK.

## Bundle commands

```text
okf init
okf validate
okf list
okf get <id-or-alias>
okf inspect <id-or-alias>
okf search <query>
okf graph
```

Use `--bundle <directory>` to select a core bundle and `--output json` for a stable machine-readable envelope.

```bash
okf --bundle ./knowledge --output json search "runtime architecture" --tag architecture
```

## Library runtime commands

Libraries are persistently registered in a runtime registry (default `.okf/libraries.json`). Registration/install and mount state are separate.

```text
okf library add <local-directory-or-git-url> [--id <id>] [--name <name>] [--ref <git-ref>]
okf library update <id>
okf library remove <id>
okf library mount <id>
okf library unmount <id>
okf library list
okf library catalog [id]
okf library read okf://<library>/<path>
okf library query <query> [--library <id>] [--limit <n>]
```

Local directories are mounted live. Git Libraries are cloned into a registry-managed cache and can be updated independently. Both are resolved into the same SDK `LibraryProvider` runtime contract; CLI routing does not branch on storage technology after resolution.

The global catalog is dynamically derived from mounted Libraries. Each Library contributes its own semantic catalog, so specialized knowledge packages can define optimized navigation instead of exposing only a root directory.

## Exit codes

- `0`: command completed and validation policy passed.
- `1`: validation diagnostics failed the selected policy.
- `2`: command-line usage error.
- `3`: operational or output error.

## SDK dependency

Until the SDK is published to crates.io, `vendor/okf` contains a source snapshot derived from `JarynXu/okf-sdk`. CLI command handlers call that dependency rather than reimplementing OKF parsing, graph traversal, retrieval, or Library runtime semantics. The CLI owns only persistence/materialization concerns such as the local registry file and invoking Git for Git sources.

## Status

The `0.1.0-alpha` APIs and JSON schemas may evolve before the first stable release.
