# OKF CLI

`okf` is the command-line interface for Open Knowledge Format bundles and pluggable OKF Libraries. Libraries extend the existing OKF knowledge space rather than introducing a second consumption API. The CLI remains a thin adapter over the `okf` Rust SDK.

## Knowledge commands

```text
okf init
okf validate
okf list
okf get <id-or-alias-or-okf-uri>
okf inspect <id-or-alias>
okf search <query> [--library <id>]
okf graph
```

Use `--bundle <directory>` to select the ordinary bundle, `--registry <path>` to select the persistent Library registry, and `--output json` for a stable machine-readable envelope.

Without mounted Libraries, these commands keep their original behavior. After Libraries are mounted, the same commands remain the normal interface:

```bash
okf --bundle ./knowledge --registry ./.okf/libraries.json search "runtime architecture"
okf --registry ./.okf/libraries.json search "XCAP document selector" --library mcx
okf --registry ./.okf/libraries.json get okf://mcx/interfaces/xcap
```

`search` without `--library` searches the active knowledge space: the current bundle plus mounted Libraries. Library-owned catalogs, routing hints, and provider capabilities may optimize the internal retrieval path. `--library` is optional advanced scoping, not a separate Library mode.

## Library management commands

Libraries are persistently registered in a runtime registry (default `.okf/libraries.json`). Registration/install and mount state are separate.

```text
okf library add <local-directory-or-git-url> [--id <id>] [--name <name>] [--ref <git-ref>]
okf library update <id>
okf library remove <id>
okf library mount <id>
okf library unmount <id>
okf library list
```

The `library` command group is the management plane. Knowledge retrieval stays on `search` and `get` whether Libraries exist or not.

Local directories are mounted live. Git Libraries are cloned into a registry-managed cache and can be updated independently. Both resolve into the same SDK `LibraryProvider` contract; after resolution, knowledge consumption does not branch on storage technology.

Each Library may contribute semantic catalog/routing metadata and a provider-specific retrieval strategy. Those are Runtime internals used to improve ordinary `search`; users do not need a separate catalog/query command sequence.

## Architectural boundary

Generic OKF tooling is domain-neutral. Installing a concrete Library must never cause `okf` itself to gain application-specific commands. Domain lifecycle, special actions, and domain-specific Agent instructions belong to that Library/application package.

## Exit codes

- `0`: command completed and validation policy passed.
- `1`: validation diagnostics failed the selected policy.
- `2`: command-line usage error.
- `3`: operational or output error.

## SDK dependency

Until the SDK is published to crates.io, `vendor/okf` contains a source snapshot derived from `JarynXu/okf-sdk`. CLI command handlers call that dependency rather than reimplementing OKF parsing, graph traversal, retrieval, or Library runtime semantics. The CLI owns only persistence/materialization concerns such as the local registry file and invoking Git for Git sources.

## Status

The `0.1.0-alpha` APIs and JSON schemas may evolve before the first stable release.
