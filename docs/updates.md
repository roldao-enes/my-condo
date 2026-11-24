# Releasing updates

Installed apps learn about new versions from a **signed manifest** published with each release. There
are no app stores and no OS code-signing certificates involved; trust comes from one Ed25519 key that
only you hold, whose public half is compiled into the app (`crates/condo-update/update-public-key.txt`).
An app only believes a `latest.json` whose `latest.json.sig` verifies against that key, so a hijacked
download host cannot push a fake update. The app **never installs anything by itself**: it tells the
user a version is available and links to it. (Updating the data is separate and always safe: the app
snapshots the database before any schema upgrade, see `docs/packaging.md`.)

## One-time setup

1. Generate the key pair (the private key goes to a file only you can read, and is never overwritten):

   ```sh
   cargo run -p condo-update --bin condo-release -- keygen ~/condo-update-private.key
   ```

2. Paste the **public key** it prints into `crates/condo-update/update-public-key.txt` and commit it.
   Until a key is there, builds do not check for updates at all.
3. Store the **private key** twice: in a password manager (this is the only copy that matters) and as
   the repository secret `UPDATE_SIGNING_KEY` (Settings > Secrets and variables > Actions; paste the
   file's content). Prefer an environment with required reviewers for it if you have collaborators.

## Publishing a release

1. Set the new version in `crates/condo-ui/Cargo.toml` (the packaging scripts read it there) and merge.
2. Tag exactly that version and push the tag: `git tag v0.2.0 && git push origin v0.2.0`.
3. CI builds and tests every platform, then creates the release with the installers plus `latest.json`
   and `latest.json.sig`, and checks the signature with the committed public key before publishing.
   It stops if the tag differs from the Cargo version or the secret is missing.

Merges to `main` still publish a `v<version>-build.<n>` release for testing; those are marked
**pre-release**, so the address apps check (`.../releases/latest/download/latest.json`) ignores them.

### Forcing an update

Set the repository variable `UPDATE_MINIMUM_VERSION` (Settings > Secrets and variables > Actions >
Variables) before tagging. Apps older than it show the notice as required rather than optional.
Clear it again afterwards, otherwise it applies to every later release.

## If something goes wrong

* **A bad release is out:** mark it as a pre-release or delete it on GitHub; `latest` then points to
  the previous tagged release again. Publish a fixed version with a higher number.
* **Private key leaked or lost:** add a new public key as an extra line in `update-public-key.txt`,
  release a version signed with the *old* key (if you still have it) so apps learn the new key, and
  only then sign with the new one. Apps accept a signature from any listed key. If the old key was
  leaked, assume installed apps must be updated by hand once, since a leaked key can sign anything.

## Manifest format

`latest.json` (schema 1): `version`, `published`, optional `notes_url` and `minimum_version`, and
`assets[]` with `platform` (`macos`, `linux-x86_64`, `linux-aarch64`, `windows-x86_64`), `name`, `url`,
`sha256` and `size`. The signature is over the exact bytes of the file. The tool is
`condo-release` (`keygen`, `manifest`, `sign`, `verify`); run it without arguments for usage.
