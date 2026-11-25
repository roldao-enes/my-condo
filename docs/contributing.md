# Contributing

## Commit messages: Conventional Commits

Every commit follows [Conventional Commits](https://www.conventionalcommits.org):

```
<type>(<optional scope>)<!>: <description>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.

Enable the local check once per clone (it rejects a bad message when you commit):

```sh
git config core.hooksPath .githooks
```

CI (`Commits` workflow) checks every commit of a pull request too, so the hook is a convenience,
not the only gate. Fix a bad message with `git commit --amend` (last commit) or an interactive
rebase, then force-push the branch.

## Versions are automatic

Versions follow [SemVer](https://semver.org) (`MAJOR.MINOR.PATCH`) and are never edited by hand. On
every push to `main` the `Release` workflow looks at the commits since the last `vX.Y.Z` tag:

| Commits since the last release                       | Next version |
|------------------------------------------------------|--------------|
| a breaking change (`type!:` or a `BREAKING CHANGE:` footer) | MAJOR (`1.4.2` -> `2.0.0`) |
| at least one `feat:`                                 | MINOR (`1.4.2` -> `1.5.0`) |
| at least one `fix:`                                  | PATCH (`1.4.2` -> `1.4.3`) |
| only `docs`, `chore`, `ci`, `test`, `refactor`, ...  | no release   |

The highest level wins. When there is a release, the workflow sets the version in every
`crates/*/Cargo.toml` and in `Cargo.lock`, commits `chore(release): vX.Y.Z`, tags it and starts the
Desktop workflow on the tag, which builds, signs and publishes it (see `docs/updates.md`).

Preview the next version locally with `scripts/release/next-version.sh` (prints nothing if no
release is due).

Note: while the version is `0.x`, SemVer treats everything as unstable, but a breaking-change
commit still bumps MAJOR (`0.x` -> `1.0.0`). Use `!` / `BREAKING CHANGE:` deliberately.
