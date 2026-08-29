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

Without mounted Libraries these commands keep their original behavior. After Libraries are mounted, the same commands remain the normal interface:

```bash
okf --bundle ./knowledge --registry ./.okf/libraries.json search "runtime architecture"
okf --registry ./.okf/libraries.json search "XCAP document selector" --library mcx
okf --registry ./.okf/libraries.json get okf://mcx/interfaces/xcap
```

`search` without `--library` searches the active knowledge space: the current bundle plus mounted Libraries. Library-owned catalogs, routing hints, and provider capabilities may optimize retrieval internally. `--library` is optional advanced scoping, not a separate Library mode.

## Library management

Libraries are persistently registered in a Runtime registry (default `.okf/libraries.json`). Installation and mount state are separate.

```text
okf library add <local-directory-or-git-url> [--id <id>] [--name <name>] [--ref <git-ref>]
okf library update <id>
okf library remove <id>
okf library mount <id> [--allow-provider <kind> ...]
okf library unmount <id>
okf library list
```

Local directories are mounted live. Git Libraries are cloned into a registry-managed cache and can be updated independently. A package may also declare provider deployments in `okf-library.yaml`.

Provider declarations are inert at install/update time. A provider kind that can execute code or access a remote service must be explicitly authorized when mounting, for example:

```bash
okf library mount project-context --allow-provider process
okf library mount remote-mcx --allow-provider http
```

Approvals are persisted only in the local Runtime registry. They are not portable package authority. Process providers run with a restricted environment; a package must explicitly name any environment variables it needs inherited. HTTP credentials are resolved from a deployment environment variable named by `token_env`; secret values do not belong in `okf-library.yaml`.

The reference CLI activates `process` and `http` deployments directly. Other storage/query adapters such as S3, SQLite, vector semantic search, or agent-backed retrieval are SDK adapters and can also be exposed through the language-neutral `okf-provider/1` process/HTTP bridge. This keeps deployment policy out of the Library domain model.

## Architectural boundary

The `library` command group is the management plane. Knowledge retrieval stays on `search` and `get` whether Libraries exist or not. Generic OKF tooling is domain-neutral: installing a Project Context, MCX, DDD, or another concrete Library must never add domain-specific commands to `okf` itself.

## Exit codes

- `0`: command completed and validation policy passed.
- `1`: validation diagnostics failed the selected policy.
- `2`: command-line usage error.
- `3`: operational or output error.

## SDK dependency

Until the SDK is published to crates.io, `vendor/okf` contains a source snapshot derived from `JarynXu/okf-sdk`. CLI handlers delegate parsing, provider protocol, provider composition, retrieval, and Library Runtime behavior to that SDK snapshot. The CLI owns deployment state and acquisition policy such as the local registry, Git materialization, and explicit provider authorization.

## Status

The `0.2.0-alpha` APIs and JSON schemas may evolve before the first stable release.
