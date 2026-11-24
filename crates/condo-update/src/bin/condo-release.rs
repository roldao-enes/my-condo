//! Release-pipeline tool: `keygen`, `manifest`, `sign` and `verify` (see `docs/updates.md`).
//! Errors go to stderr with a non-zero exit code so CI stops.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use condo_update::release::{build_manifest, to_bytes, ManifestRequest};
use condo_update::signing::{generate_keypair, open, parse_public_keys, sign};

const USAGE: &str = "\
condo-release keygen <private-key-file>
    Makes a key pair: writes the private key to the file (refuses to overwrite) and prints the
    public key for crates/condo-update/update-public-key.txt.
condo-release manifest --version V --published TIME --base-url URL [--notes-url URL]
                       [--minimum-version V] --out FILE <release files...>
condo-release sign --key-file FILE | --key-env VAR  <file>     (writes <file>.sig)
condo-release verify --public-keys FILE <manifest> <signature-file>";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// `--name value` options and the remaining positional arguments.
struct Args {
    options: Vec<(String, String)>,
    positional: Vec<String>,
}

impl Args {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut parsed = Args {
            options: Vec::new(),
            positional: Vec::new(),
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if let Some(name) = arg.strip_prefix("--") {
                let value = args.next().ok_or(format!("--{name} needs a value"))?;
                parsed.options.push((name.to_owned(), value));
            } else {
                parsed.positional.push(arg);
            }
        }
        Ok(parsed)
    }

    fn get(&self, name: &str) -> Option<String> {
        self.options
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    }

    fn require(&self, name: &str) -> Result<String, String> {
        self.get(name)
            .ok_or(format!("--{name} is required\n{USAGE}"))
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let (command, rest) = args.split_first().ok_or(USAGE.to_owned())?;
    let args = Args::parse(rest.to_vec())?;
    match command.as_str() {
        "keygen" => {
            let [file] = args.positional.as_slice() else {
                return Err(USAGE.to_owned());
            };
            let (private, public) = generate_keypair();
            write_private_key(file, &private)?;
            println!("Private key written to {file}. Keep it secret; never commit it.");
            println!("Public key (add it to crates/condo-update/update-public-key.txt):");
            println!("{public}");
            Ok(())
        }
        "manifest" => {
            let request = ManifestRequest {
                version: args.require("version")?,
                published: args.require("published")?,
                base_url: args.require("base-url")?,
                notes_url: args.get("notes-url"),
                minimum_version: args.get("minimum-version").filter(|v| !v.is_empty()),
                files: args.positional.iter().map(PathBuf::from).collect(),
            };
            let out = args.require("out")?;
            let manifest = build_manifest(&request)?;
            fs::write(&out, to_bytes(&manifest)).map_err(|e| format!("{out}: {e}"))?;
            println!(
                "{out}: version {} with {} files",
                manifest.version,
                manifest.assets.len()
            );
            Ok(())
        }
        "sign" => {
            let [file] = args.positional.as_slice() else {
                return Err(USAGE.to_owned());
            };
            let key = match (args.get("key-file"), args.get("key-env")) {
                (Some(path), None) => {
                    fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?
                }
                (None, Some(var)) => {
                    std::env::var(&var).map_err(|_| format!("${var} is not set"))?
                }
                _ => {
                    return Err(format!(
                        "give exactly one of --key-file / --key-env\n{USAGE}"
                    ))
                }
            };
            let bytes = fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            let signature = sign(&key, &bytes).map_err(|e| e.to_string())?;
            let out = format!("{file}.sig");
            fs::write(&out, format!("{signature}\n")).map_err(|e| format!("{out}: {e}"))?;
            println!("{out}");
            Ok(())
        }
        "verify" => {
            let [manifest, signature] = args.positional.as_slice() else {
                return Err(USAGE.to_owned());
            };
            let keys_path = args.require("public-keys")?;
            let keys = parse_public_keys(
                &fs::read_to_string(&keys_path).map_err(|e| format!("{keys_path}: {e}"))?,
            )
            .map_err(|e| e.to_string())?;
            let bytes = fs::read(manifest).map_err(|e| format!("{manifest}: {e}"))?;
            let signature =
                fs::read_to_string(signature).map_err(|e| format!("{signature}: {e}"))?;
            let verified = open(&bytes, &signature, &keys).map_err(|e| e.to_string())?;
            println!("signature OK: version {}", verified.version);
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}

/// Creates the file readable by its owner only, and never overwrites an existing key.
fn write_private_key(path: &str, key: &str) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .map_err(|e| format!("{path}: {e} (an existing key is never overwritten)"))?;
    writeln!(file, "{key}").map_err(|e| format!("{path}: {e}"))
}
