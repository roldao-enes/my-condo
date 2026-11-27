# Upgrade fixtures

`v<N>.db` is a populated database as schema version `N` leaves it. The unit tests in
`src/upgrade_tests.rs` open every one with the current code and check nothing is lost.

* **When you add a migration (schema version N), add `v<N>.db` in the same change**, or
  `every_schema_version_has_a_fixture` fails.
* **Never regenerate or edit an existing fixture.** It stands for data that real users already
  have. If a test on an old fixture fails, the migration is wrong, not the fixture.
* `seed.sql` is the sample data every fixture starts from; `seed_v<N>.sql` adds what schema N
  introduced. Keep seeds minimal but touching every table, with accents and NULLs.
* Fixtures are single files (rollback journal, not WAL) and marked `binary` in `.gitattributes`.

Building a fixture for a new schema version N (from the repository root; the migrations are
applied in order, then the seeds, then the version is stamped):

```sh
cd crates/condo-db
F=tests/fixtures/upgrade
{ cat migrations/0001_*.sql migrations/0002_*.sql ... migrations/000N_*.sql \
    $F/seed.sql $F/seed_v2.sql ... $F/seed_vN.sql
  echo "PRAGMA user_version = N;"; } | sqlite3 $F/vN.db
```

**When a migration renames a table or column, or changes a stored value**, `seed.sql` (written for
schema v1) no longer applies after it. Build that fixture in two steps instead: apply the
migrations before it, `seed.sql` and the earlier `seed_v<N>.sql`; then apply the renaming migration
and `seed_vN.sql`, which is written with the new names (this is how `v3.db` was made). Also declare
the change in `RENAMES` in `src/upgrade_tests.rs` (renamed tables and columns, changed values, and
tables a migration appends rows to) so the old fixtures can still be compared with the upgraded
data. That list is the only way the data comparison is relaxed; nothing else may change. Use
`PRAGMA foreign_keys = ON;` first, because the `sqlite3` shell of some systems defaults to the
legacy `ALTER TABLE` behaviour.

Ideally build the new fixture from the **released** app (run the previous release, enter data,
copy `condo.db`) rather than from the migrations, so it also covers what the migrations did not
anticipate.
