---
name: kget-quality
description: Use when improving KGet tests, reliability, download correctness, resumability, cancellation, CI, release readiness, or performance.
---

# KGet Quality Skill

KGet handles user files and network transfers, so reliability beats flashy
changes. Focus on observable correctness and recoverability.

## Checklist

- Cover range downloads, fallback without ranges, retry behavior, and cancellation.
- Preserve partially downloaded files only when they are resumable and valid.
- Verify output paths, disk space, and unsafe filenames.
- Avoid logging credentials, proxy passwords, cookies, or bearer tokens.
- Prefer structured errors that frontends can display safely.
- Keep tests network-isolated unless explicitly marked as live/integration.

## Commands

```bash
cargo test --lib
cargo test --test unit_tests
cargo test --test mock_server_tests
```

For release confidence:

```bash
cargo test
cargo check --features gui
```

