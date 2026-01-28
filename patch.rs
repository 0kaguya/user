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
toml = "0.8"

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
    Toml(TomlConfig),
    Text(String),
    #[default] None,
}

struct TomlConfig {
    value: toml::Value,
}

impl Default for TomlConfig {
    fn default() -> Self {
        TomlConfig {
            value: toml::Value::Table(Default::default()),
        }
    }
}

impl ToString for TomlConfig {
    fn to_string(&self) -> String {
        toml::to_string_pretty(&self.value)
            .expect("Serialize a toml::Value should not fail")
    }
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
            Some("toml") => Ok(Config::Toml(TomlConfig {
                value: toml::from_str(&text)
                    .context(format!("Possible empty toml: `{text}`"))?,
            })),
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
            (Toml(a), Toml(b)) => Ok(Toml(a.merge(b))),
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
            Toml(toml) => toml.to_string(),
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

impl Mergeable for TomlConfig {
    fn merge(self, other: Self) -> Self {
        fn merge_values(a: toml::Value, b: toml::Value) -> toml::Value {
            match (a, b) {
                (toml::Value::Table(mut left), toml::Value::Table(right)) => {
                    for (k, v) in right {
                        let merged = if let Some(existing) = left.remove(&k) {
                            merge_values(existing, v)
                        } else {
                            v
                        };
                        left.insert(k, merged);
                    }
                    toml::Value::Table(left)
                }
                (_, b) => b,
            }
        }

        TomlConfig {
            value: merge_values(self.value, other.value),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Test code are mostly AI-generated.
    use super::*;
    use std::io::Write;
    use tempfile::{tempdir, tempdir_in, Builder, TempDir};

    struct TestEnv {
        _root: TempDir,
        patch_dir: TempDir,
        target_dir: TempDir,
    }

    impl TestEnv {
        fn new() -> Self {
            let root = tempdir().unwrap();
            let patch_dir = tempdir_in(root.path()).unwrap();
            let target_dir = tempdir_in(root.path()).unwrap();
            
            TestEnv {
                _root: root,
                patch_dir,
                target_dir,
            }
        }

        fn create_patch_dir(&self, name: &str) -> PathBuf {
            let d = self.patch_dir.path().join(name);
            std::fs::create_dir_all(&d).unwrap();
            d
        }

        fn write_patch_file(&self, dir: &Path, prefix: &str, content: &[u8]) {
            let temp_file = Builder::new().prefix(prefix).tempfile_in(dir).unwrap();
            let (mut file, _) = temp_file.keep().unwrap();
            file.write_all(content).unwrap();
        }

        fn write_named_patch_file(&self, dir: &Path, name: &str, content: &[u8]) {
            let mut file = std::fs::File::create(dir.join(name)).unwrap();
            file.write_all(content).unwrap();
        }

        fn run_patch(&self) {
            super::start(Cli {
                directory: self.patch_dir.path().to_path_buf(),
                target: self.target_dir.path().to_path_buf(),
                log_level: log::Level::Error,
            }).unwrap();
        }

        fn read_target_file(&self, name: &str) -> String {
            std::fs::read_to_string(self.target_dir.path().join(name)).unwrap()
        }

        fn read_target_json(&self, name: &str) -> serde_json::Value {
            serde_json::from_str(&self.read_target_file(name)).unwrap()
        }

        fn read_target_toml(&self, name: &str) -> toml::Value {
            toml::from_str(&self.read_target_file(name)).unwrap()
        }
    }

    #[test]
    fn argparse_test() {
        use std::env;
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
        let env = TestEnv::new();
        let d = env.create_patch_dir("dot-foo.d");

        env.write_patch_file(&d, "000", b"hello");
        env.write_patch_file(&d, "001", b"world");
        env.run_patch();

        let result = env.read_target_file(".foo");
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
    }

    #[test]
    fn json_test() {
        let env = TestEnv::new();
        let d = env.create_patch_dir("dot-bar.json.d");

        env.write_patch_file(&d, "000", br#"{"foo": "bar"}"#);
        env.write_patch_file(&d, "001", br#"{"baz": "qux"}"#);
        env.run_patch();

        let result = env.read_target_json(".bar.json");
        assert_eq!(result["foo"], "bar");
        assert_eq!(result["baz"], "qux");
    }

    #[test]
    fn json_merge_test() {
        let env = TestEnv::new();
        let d = env.create_patch_dir("dot-baz.json.d");

        env.write_patch_file(&d, "000", br#"{"a":1,"nested":{"x":1}}"#);
        env.write_patch_file(&d, "001", br#"{"b":2,"nested":{"y":2}}"#);
        env.run_patch();

        let result = env.read_target_json(".baz.json");
        assert_eq!(result["a"], 1);
        assert_eq!(result["b"], 2);
        assert_eq!(result["nested"]["x"], 1);
        assert_eq!(result["nested"]["y"], 2);
    }

    #[test]
    fn jsonc_test() {
        let env = TestEnv::new();
        let d = env.create_patch_dir("dot-jsonc.json.d");

        env.write_patch_file(&d, "000", b"{\n  // line comment\n  \"foo\": 1,\n}\n");
        env.write_patch_file(&d, "001", b"{\n  /* block comment */\n  \"bar\": 2\n}\n");
        env.run_patch();

        let result = env.read_target_json(".jsonc.json");
        assert_eq!(result["foo"], 1);
        assert_eq!(result["bar"], 2);
    }

    #[test]
    fn toml_test() {
        let env = TestEnv::new();
        let d = env.create_patch_dir("dot-toml.toml.d");

        env.write_named_patch_file(&d, "000.toml", b"foo = \"bar\"\n[nested]\nx = 1\n");
        env.write_named_patch_file(&d, "001.toml", b"baz = \"qux\"\n[nested]\ny = 2\n");
        env.run_patch();

        let result = env.read_target_toml(".toml.toml");
        assert_eq!(result["foo"].as_str().unwrap(), "bar");
        assert_eq!(result["baz"].as_str().unwrap(), "qux");
        assert_eq!(result["nested"]["x"].as_integer().unwrap(), 1);
        assert_eq!(result["nested"]["y"].as_integer().unwrap(), 2);
    }

    #[test]
    fn ignore_test() {
        let env = TestEnv::new();
        let d = env.create_patch_dir("dot-ignore.d");

        env.write_named_patch_file(&d, "000", b"keep-this");
        env.write_named_patch_file(&d, "AGENTS.md", b"secret");
        env.write_named_patch_file(&d, "README.md", b"topline");
        env.run_patch();

        let result = env.read_target_file(".ignore");
        assert!(result.contains("keep-this"));
        assert!(!result.contains("secret"));
        assert!(!result.contains("topline"));
    }
}
