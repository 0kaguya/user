#!/usr/bin/env -S cargo +nightly -Zscript
---cargo
package.edition = "2024"

[dependencies]
anyhow = "1.0.100"
clap = { version = "4.5.54", features = ["derive", "env"] }
duct = "1.1.1"
json-patch = "4.1.0"
jsonc-parser = { version = "0.29.0", features = ["serde"] }
log = "0.4.29"
serde_json = "1.0.149"
simple_logger = "5.1.0"

[dev-dependencies]
tempfile = "3.24.0"
---
#![feature(iterator_try_reduce)]

use std::{io::Seek, sync::LazyLock};
static IGNORE_LIST: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "AGENTS.md", "README.md",
    ]
});

use std::path::{Path, PathBuf};
use log::Level::*;

#[derive(Debug, clap::Parser)]
/// Provides some customizations that better than nothing.
struct Cli {
    #[arg(short, long, default_value = "patches")]
    /// Path to the config house.
    directory: PathBuf,
    #[arg(long, env = "HOME")]
    /// Path to the target directory. Defaults to user home.
    target: PathBuf,

    #[arg(long, default_value_t = Info)]
    log_level: log::Level,
}

use anyhow::Result;
fn main() -> Result<()> {
    use clap::Parser;
    let cli = Cli::parse();

    use simple_logger::init_with_level;
    init_with_level(cli.log_level)?;
    log::debug!("Starting logger in `{}` mode", cli.log_level);

    start(cli)
}

/// Separated for test purpose.
fn start(cli: Cli) -> Result<()> {
    use anyhow::Context;
    use duct::cmd;

    cmd!("find", &cli.directory).pipe(cmd!("grep", "-e", r"\.d$"))
    .read()?

    .lines()
    .map(|directory| {
        fn canonicalize(directory: &str) -> String {
            directory
            .trim()
            .trim_end_matches(".d")
            .replace("dot-", ".")
        }

        fn replace_prefix(from: &Path, to: &Path, path: impl AsRef<Path>)
            -> Result<PathBuf>
        {
            Ok(to.join(
                path.as_ref().strip_prefix(from)
                    .context("Strip prefix")?
            ))
        }

        let target = replace_prefix(
            &cli.directory, &cli.target, canonicalize(directory)
        )
        .context("Get target")?;

        let format = target.extension()
            .and_then(|e| e.to_str())
            .map(str::to_string);

        return Ok((format, target, directory));
    })

    .map(|r| r.and_then(|(format, target, directory)| {
        use std::fs::OpenOptions;
        use std::iter::once;

        log::trace!("Opening {target:?}");
        let target =
            OpenOptions::new().read(true).write(true).create(true)
                .open(target)?;

        let result =
            once(target.try_clone())
            .filter(|result| result.as_ref().is_ok_and(|file| {
                file.metadata().is_ok_and(|m| m.len() > 0)
            }))
            .chain(
                cmd!("ls", &directory).read()
                    .context(format!("`ls` files in {directory}"))?
                .lines()

                .filter(|s| !IGNORE_LIST.contains(s))
                .map(|s| Path::new(directory).join(s))

                .map(|path| {
                    log::trace!("Opening {path:?}");
                    OpenOptions::new().read(true).open(&path)
                })
            )
            // Maps IO error to anyhow error.
            .map(|result| {
                result.map_err(Into::into)
            })
            // Read content of opened files.
            .map(|result| result.and_then(|mut file| {
                use std::io::Read;

                let mut buf = String::new();
                file.read_to_string(&mut buf)?;

                log::trace!("Reading {} bytes", buf.len());
                Ok(buf)
            }))
            .map(|result| result.and_then(|text| {
                Config::parse_dispatch(&format, text)
            }))
            // Concatenate with the Mereable trait.
            .try_fold(Default::default(), Config::try_merge)?;

        fn write_back(text: String, mut f: std::fs::File) -> Result<()> {
            use std::io::Write;
            // Write from file start.
            f.rewind()?;
            f.write_all(text.as_bytes())?;
            // Truncate rest content.
            let pos = f.stream_position()?;
            f.set_len(pos)?;
            Ok(())
        }

        write_back(result.into(), target)
    }))
    .collect::<Result<()>>()?;

    Ok(())
}

#[derive(Default)]
enum Config {
    Json(serde_json::Value),
    Text(String),
    #[default] None,
}

impl Config {
    fn parse_dispatch(format: &Option<String>, text: String) -> Result<Self> {
        use anyhow::{bail, Context};
        use jsonc_parser::parse_to_serde_value;

        match format.as_deref() {
            Some("json") => Ok(Config::Json(
                parse_to_serde_value(&text, &Default::default())?
                    .context(format!("Possible empty json: `{text}`"))?
            )),
            Some("text") | None => Ok(Config::Text(text)),
            _ => bail!("Unsupported format: {format:?}"),
        }
    }

    fn try_merge(self, other: Result<Self>) -> Result<Self> {
        use anyhow::bail;
        use Config::*;

        match (self, other?) {
            (None, other) => Ok(other),
            (Json(a), Json(b)) => Ok(Json(a.merge(b))),
            (Text(a), Text(b)) => Ok(Text(a.merge(b))),
            _ => bail!("Cannot merge different types"),
        }
    }
}

impl From<Config> for String {
    fn from(config: Config) -> Self {
        use Config::*;
        use serde_json::to_string_pretty;
        match config {
            Json(json) => to_string_pretty(&json)
                .expect("Serialize a serde_json::Value should not fail"),
            Text(text) => text,
            None => String::new(),
        }
    }
}

trait Mergeable: Default + ToString {
    fn merge(self, other: Self) -> Self;
}

impl Mergeable for serde_json::Value {
    fn merge(mut self, other: Self) -> Self {
        use json_patch::merge;
        merge(&mut self, &other);
        self
    }
}

impl Mergeable for String {
    fn merge(mut self, other: Self) -> Self {
        self.push('\n');
        self.push_str(&other);
        self
    }
}

#[cfg(test)]
mod tests {
    //! Test code are mostly AI-generated.
    use super::*;

    #[test]
    fn argparse_test() {
        use std::env;
        use tempfile::tempdir;
        use clap::Parser;

        let root = tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        unsafe {
            env::set_var("HOME", &home);
        }

        let cli = Cli::parse_from(["patch"]);
        assert_eq!(cli.directory, PathBuf::from("patches"));
        assert_eq!(cli.target, home);
        assert_eq!(cli.log_level, log::Level::Info);
    }

    #[test]
    fn plain_text_test() {
        use tempfile::{
            tempdir, tempdir_in, Builder,
        };
        use std::io::Write;

        let root = tempdir().unwrap();
        let patch_dir = tempdir_in(root.path()).unwrap();
        let target_dir = tempdir_in(root.path()).unwrap();

        let d = patch_dir.path().join("dot-foo.d");
        std::fs::create_dir_all(&d).unwrap();
        let mut f0 = Builder::new().prefix("000").tempfile_in(&d).unwrap();
        f0.write_all(b"hello").unwrap();
        let mut f1 = Builder::new().prefix("001").tempfile_in(&d).unwrap();
        f1.write_all(b"world").unwrap();

        super::start(Cli {
            directory: patch_dir.path().to_path_buf(),
            target: target_dir.path().to_path_buf(),
            log_level: log::Level::Error,
        }).unwrap();

        let result =
            std::fs::read_to_string(target_dir.path().join(".foo")).unwrap();
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
    }

    #[test]
    fn json_test() {
        use tempfile::{
            tempdir, tempdir_in, Builder,
        };
        use std::io::Write;

        let root = tempdir().unwrap();
        let patch_dir = tempdir_in(root.path()).unwrap();
        let target_dir = tempdir_in(root.path()).unwrap();

        let d = patch_dir.path().join("dot-bar.json.d");
        std::fs::create_dir_all(&d).unwrap();
        let mut f0 = Builder::new().prefix("000").tempfile_in(&d).unwrap();
        f0.write_all(br#"{"foo": "bar"}"#).unwrap();
        let mut f1 = Builder::new().prefix("001").tempfile_in(&d).unwrap();
        f1.write_all(br#"{"baz": "qux"}"#).unwrap();

        super::start(Cli {
            directory: patch_dir.path().to_path_buf(),
            target: target_dir.path().to_path_buf(),
            log_level: log::Level::Error,
        }).unwrap();

        let result =
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(target_dir.path().join(".bar.json")).unwrap()
            ).unwrap();
        assert_eq!(result["foo"], "bar");
        assert_eq!(result["baz"], "qux");
    }

    #[test]
    fn json_merge_test() {
        use tempfile::{
            tempdir, tempdir_in, Builder,
        };
        use std::io::Write;

        let root = tempdir().unwrap();
        let patch_dir = tempdir_in(root.path()).unwrap();
        let target_dir = tempdir_in(root.path()).unwrap();

        let d = patch_dir.path().join("dot-baz.json.d");
        std::fs::create_dir_all(&d).unwrap();
        let mut f0 = Builder::new().prefix("000").tempfile_in(&d).unwrap();
        f0.write_all(br#"{"a":1,"nested":{"x":1}}"#).unwrap();
        let mut f1 = Builder::new().prefix("001").tempfile_in(&d).unwrap();
        f1.write_all(br#"{"b":2,"nested":{"y":2}}"#).unwrap();

        super::start(Cli {
            directory: patch_dir.path().to_path_buf(),
            target: target_dir.path().to_path_buf(),
            log_level: log::Level::Error,
        }).unwrap();

        let result =
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(target_dir.path().join(".baz.json")).unwrap()
            ).unwrap();
        assert_eq!(result["a"], 1);
        assert_eq!(result["b"], 2);
        assert_eq!(result["nested"]["x"], 1);
        assert_eq!(result["nested"]["y"], 2);
    }

    #[test]
    fn jsonc_test() {
        use tempfile::{
            tempdir, tempdir_in, Builder,
        };
        use std::io::Write;

        let root = tempdir().unwrap();
        let patch_dir = tempdir_in(root.path()).unwrap();
        let target_dir = tempdir_in(root.path()).unwrap();

        let d = patch_dir.path().join("dot-jsonc.json.d");
        std::fs::create_dir_all(&d).unwrap();
        let mut f0 = Builder::new().prefix("000").tempfile_in(&d).unwrap();
        f0.write_all(b"{\n  // line comment\n  \"foo\": 1,\n}\n").unwrap();
        let mut f1 = Builder::new().prefix("001").tempfile_in(&d).unwrap();
        f1.write_all(b"{\n  /* block comment */\n  \"bar\": 2\n}\n").unwrap();

        super::start(Cli {
            directory: patch_dir.path().to_path_buf(),
            target: target_dir.path().to_path_buf(),
            log_level: log::Level::Error,
        }).unwrap();

        let result =
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(target_dir.path().join(".jsonc.json")).unwrap()
            ).unwrap();
        assert_eq!(result["foo"], 1);
        assert_eq!(result["bar"], 2);
    }

    #[test]
    /// Test on super::IGNORE_LIST.
    fn ignore_test() {
        use tempfile::{
            tempdir, tempdir_in,
        };
        use std::io::Write;

        let root = tempdir().unwrap();
        let patch_dir = tempdir_in(root.path()).unwrap();
        let target_dir = tempdir_in(root.path()).unwrap();

        let d = patch_dir.path().join("dot-ignore.d");
        std::fs::create_dir_all(&d).unwrap();
        let mut keep = std::fs::File::create(d.join("000")).unwrap();
        keep.write_all(b"keep-this").unwrap();
        let mut ignored1 = std::fs::File::create(d.join("AGENTS.md")).unwrap();
        ignored1.write_all(b"secret").unwrap();
        let mut ignored2 = std::fs::File::create(d.join("README.md")).unwrap();
        ignored2.write_all(b"topline").unwrap();

        super::start(Cli {
            directory: patch_dir.path().to_path_buf(),
            target: target_dir.path().to_path_buf(),
            log_level: log::Level::Error,
        }).unwrap();

        let result =
            std::fs::read_to_string(target_dir.path().join(".ignore")).unwrap();
        assert!(result.contains("keep-this"));
        assert!(!result.contains("secret"));
        assert!(!result.contains("topline"));
    }
}
