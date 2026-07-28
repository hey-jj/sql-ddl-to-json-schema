# Changelog

## [0.2.1] - 2026-07-28

### Fixed
- Corrected `rust-version` from 1.74 to 1.85.0. The declared floor was never reachable: the
  `preserve_order` feature this crate requests from `serde_json` pulls in `indexmap`, which requires
  Rust 1.85.0. Builds on 0.1.0 and 0.2.0 fail below that despite the metadata. No code changed.

## [0.2.0] - 2026-07-07

### Changed
- `ALTER TABLE ... ADD SPATIAL INDEX` now lists indexes in `spatialIndexes` instead of `fulltextIndexes` in compact output. (#13)
- SQL statement splitting now ignores semicolons inside hash comments, dash comments, and block comments, including comments split across feed chunks. (#14)
- `SET` statements with quoted string assignments, such as `SET sql_mode='ANSI';`, now parse as `P_SET` statements. (#15)
- JSON Schema patterns for SQL `SET` values now treat regex metacharacters as literal characters. (#16)
- `ALTER TABLE` column moves that place a column after itself now leave column order unchanged. (#17)
