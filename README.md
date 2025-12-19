# candid-language-server

Experimental language server for [Candid](https://github.com/dfinity/candid).

## Configuration

The server reads user preferences via `workspace/configuration`. Editors can send a
`candidLanguageServer` section with:

- `serviceSnippets.style`: Controls completion snippets for service methods.
  Accepted values are `"call"` (default), `"await"`, `"async"`, or `"await-let"`.
- `completion.mode`: `"full"`, `"lightweight"`, or `"auto"` (default). Auto switches to
  lightweight mode when a document exceeds 2,000 lines or 120k characters.
- `completion.auto.lineLimit` / `completion.auto.charLimit`: Override auto mode thresholds.
  Limits must be positive integers.
- `format.enabled`: Enable or disable formatting (default: `true`).
- `format.indentWidth`: Override indentation width applied after formatting. Must be a
  positive integer. If omitted, keeps formatter output unchanged.
- `format.blankLines`: Maximum consecutive blank lines to keep after formatting. If omitted,
  keeps formatter output unchanged.

These keys accept `camelCase`, `snake_case`, or `kebab-case` variants.

Example VS Code-style settings JSON:

```json
{
  "candidLanguageServer": {
    "serviceSnippets": {
      "style": "await"
    },
    "completion": {
      "mode": "auto",
      "auto": {
        "lineLimit": 3000
      }
    },
    "format": {
      "enabled": true,
      "indentWidth": 2,
      "blankLines": 1
    }
  }
}
```

## Benchmarking and tracing

- Run `cargo bench --features bench` to execute the Criterion-based completion benchmarks. The harness preloads `tests/data/hover_sample.did` so you can observe relative improvements without wiring up an editor.
- Enable detailed tracing with `cargo run --features tracing -- ...` (or the equivalent command your editor uses). `tracing-subscriber` honors `RUST_LOG=completion=trace` so you can capture per-phase timings for completion requests.
- Runtime completion requests are processed through a cancellable async pipeline. Every URI owns a `CompletionJobState`; each request grabs a fresh token so newer edits immediately invalidate older work. The builder periodically `yield_now()` and checks the token, so large files are produced in short slices without blocking user input.

## Data ownership model

- Each document URI maps to a single `DocumentSnapshot` (rope + optional version). Incremental edits update this snapshot atomically so hover/completion operate on the same rope instance.
- Parsed/semantic artifacts live in one `AnalysisSnapshot` per URI. The snapshot owns the AST, semantic analysis result, and the completion cache for the current document version, guaranteeing all features share the same data.
- When a document change lands, the previous snapshots are dropped and rebuilt once, so hover/completion/diagnostics never attempt to rebuild caches per request.
- Lightweight completion mode (auto-enabled for very large files) only reads from these snapshots to offer locals, keywords, and service labels while skipping expensive snippet synthesis and field aggregation.

## License

This project is licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or [MIT License](./LICENSE-MIT) at your option.
