# Releasing updates

Installed apps learn about new versions from a **signed manifest** published with each release. There
are no app stores and no OS code-signing certificates involved; trust comes from one Ed25519 key that
only you hold, whose public half is compiled into the app (`crates/condo-update/update-public-key.txt`).
An app only believes a `latest.json` whose `latest.json.sig` verifies against that key, so a hijacked
download host cannot push a fake update. The app **never installs anything by itself**: it tells the
user a version is available and links to the release page. (Updating the data is separate and always safe: the app
snapshots the database before any schema upgrade, see `docs/packaging.md`.)

## What the app does

* After sign-in, and then at most once every 24 hours while it is open, it downloads
  `releases/latest/download/latest.json` and its `.sig` (a few hundred bytes; plain GETs of public
  files that carry nothing about the person or the condominium), verifies the signature and compares
  versions. A newer version shows a banner with **Ver novidades** (opens the release page in the
  browser) and **Mais tarde** (hides that version until a newer one appears). If the running version
  is below `minimum_version`, the banner cannot be put off.
* **Definições > Atualizações** has a checkbox to turn the automatic check off, a **Procurar agora**
  button, and the installed version. Failures of the automatic check are silent (being offline is
  normal); a manual check says what went wrong.
* Builds without a public key in `update-public-key.txt` never contact the network for this.
* When starting a new version upgrades the database, a notice after sign-in says a safety copy of
  the previous state is in the `backups` folder.

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
