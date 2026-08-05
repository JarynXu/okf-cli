# JSON output

Every successful command emits:

```json
{
  "schema_version": "1",
  "ok": true,
  "command": "search",
  "data": {}
}
```

Usage and operational errors use the same envelope with `ok: false` and an `error` object. Human diagnostics are sent to stderr; JSON diagnostics are sent to stdout so agents can parse them deterministically.
