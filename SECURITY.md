# Security Policy

## Supported Versions

This is the versions of the program that are
currently being supported with security updates.

| Version | Supported          |
| ------- | ------------------ |
| 1.8.x   | :white_check_mark: |
| 1.7.x   | :white_check_mark: |
| 0.1.2   | :x:                |
| 0.1.1   | :x:                |
| 0.1.0   | :x:                |

## Dependency review

The release lockfile uses `librqbit` 9.0.1 for the optional native torrent
client. Its uTP dependency currently pulls `ringbuf` 0.4.8, for which
RUSTSEC-2026-0293 is listed. The vulnerable operation requires an element's
`Drop` implementation to panic. `librqbit-utp` uses that ring buffer only as
`SharedRb<Heap<u8>>`; `u8` has no `Drop` implementation, so this condition is
not reachable in KGet's dependency graph. The narrowly scoped `audit.toml`
exception is intentional, and must be removed when librqbit updates its uTP
dependency.
