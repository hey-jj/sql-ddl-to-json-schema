# Changelog

## [0.2.0] - 2026-07-07

### Changed
- `ALTER TABLE ... ADD SPATIAL INDEX` now lists indexes in `spatialIndexes` instead of `fulltextIndexes` in compact output. (#13)
- SQL statement splitting now ignores semicolons inside hash comments, dash comments, and block comments, including comments split across feed chunks. (#14)
- `SET` statements with quoted string assignments, such as `SET sql_mode='ANSI';`, now parse as `P_SET` statements. (#15)
- JSON Schema patterns for SQL `SET` values now treat regex metacharacters as literal characters. (#16)
- `ALTER TABLE` column moves that place a column after itself now leave column order unchanged. (#17)
