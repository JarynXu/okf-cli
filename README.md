# OKF CLI

`okf` is the command-line interface for Open Knowledge Format bundles, pluggable OKF Libraries, and repository-bound Project Context recovery. It remains a thin, deterministic adapter over the `okf` Rust SDK plus application adapters for persistence, Git materialization, and freshness state.

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

## Project Context commands

Project Context is an application profile built on the Library Runtime for durable project knowledge across sessions and subagents. Runtime-local profile state defaults to `.okf/project-context.json`; the generated Library scaffold defaults to `.okf/project-context/`.

```text
okf project init [--repository <git-repository>] [--project <name>] [--id <library-id>]
okf project status
okf project checkpoint [--revision <verified-commit>]
```

`project init` creates a standard OKF Library containing current architecture, constraints, decisions, components, and append-only history, then registers and mounts it. `project status` compares the last validated revision with repository `HEAD` and returns `UNINITIALIZED`, `VALID`, `DIRTY`, or `UNKNOWN`, along with changed paths and impacted knowledge topics when the Git delta can be established.

When status is `DIRTY`, Agents should revalidate the impacted knowledge frontier rather than automatically relearning the repository. `project checkpoint` only records a revision that has already passed the caller's required project tests, review, and knowledge maintenance; it is deliberately not a substitute for those checks.

Use `--project-context <path>` to select another profile state file and `--output json` for Agent/tool integration.

## Exit codes

- `0`: command completed and validation policy passed.
- `1`: validation diagnostics failed the selected policy.
- `2`: command-line usage error.
- `3`: operational or output error.

## SDK dependency

Until the SDK is published to crates.io, `vendor/okf` contains a source snapshot derived from `JarynXu/okf-sdk`. CLI command handlers call that dependency rather than reimplementing OKF parsing, graph traversal, retrieval, or Library runtime semantics. The CLI owns only persistence/materialization and application-adapter concerns such as the local registry file, Git source operations, and Project Context freshness state.

## Status

The `0.1.0-alpha` APIs and JSON schemas may evolve before the first stable release.
