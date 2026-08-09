use std::env;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const DEFAULT_REPOSITORY: &str = "andy5090/tui-4-everything";
const DEFAULT_API_BASE: &str = "https://api.github.com";
const DEFAULT_DOWNLOAD_BASE: &str = "https://github.com";

#[derive(Debug, Clone, Default)]
pub struct UpdateRequest {
    pub check_only: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    UpToDate {
        version: Version,
    },
    CurrentIsNewer {
        current: Version,
        latest: Version,
    },
    Available {
        current: Version,
        target: Version,
    },
    Updated {
        previous: Version,
        installed: Version,
        executable: PathBuf,
    },
}

impl fmt::Display for UpdateOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UpToDate { version } => {
                write!(formatter, "T4E {version} is already up to date.")
            }
            Self::CurrentIsNewer { current, latest } => write!(
                formatter,
                "T4E {current} is newer than the latest published release ({latest}); no update was installed."
            ),
            Self::Available { current, target } => {
                write!(formatter, "T4E {target} is available (current: {current}).")
            }
            Self::Updated {
                previous,
                installed,
                executable,
            } => write!(
                formatter,
                "Updated T4E from {previous} to {installed} at {}.",
                executable.display()
            ),
        }
    }
}

pub fn run(request: UpdateRequest) -> Result<UpdateOutcome> {
    let config = UpdateConfig::from_environment()?;
    execute_update(&config, &SystemDownloader, request, |message| {
        eprintln!("{message}");
    })
}

#[derive(Debug)]
struct UpdateConfig {
    repository: String,
    api_base: String,
    download_base: String,
    current_version: Version,
    executable: PathBuf,
    asset_label: String,
}

impl UpdateConfig {
    fn from_environment() -> Result<Self> {
        let repository = environment_override("T4E_UPDATE_REPOSITORY", "T4E_INSTALL_REPOSITORY")
            .unwrap_or_else(|| DEFAULT_REPOSITORY.to_string());
        let api_base = environment_override("T4E_UPDATE_API_BASE", "T4E_INSTALL_API_BASE")
            .unwrap_or_else(|| DEFAULT_API_BASE.to_string());
        let download_base =
            environment_override("T4E_UPDATE_DOWNLOAD_BASE", "T4E_INSTALL_DOWNLOAD_BASE")
                .unwrap_or_else(|| DEFAULT_DOWNLOAD_BASE.to_string());
        let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
            .context("the built-in T4E version is invalid")?;
        let executable = env::current_exe()
            .context("could not locate the running T4E executable")?
            .canonicalize()
            .context("could not resolve the running T4E executable")?;
        let asset_label = release_asset_label(env::consts::OS, env::consts::ARCH)?;

        Ok(Self {
            repository,
            api_base: api_base.trim_end_matches('/').to_string(),
            download_base: download_base.trim_end_matches('/').to_string(),
            current_version,
            executable,
            asset_label,
        })
    }
}

fn environment_override(primary: &str, fallback: &str) -> Option<String> {
    env::var(primary)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var(fallback)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

trait Downloader {
    fn download(&self, url: &str, destination: &Path) -> Result<()>;
}

struct SystemDownloader;

impl Downloader for SystemDownloader {
    fn download(&self, url: &str, destination: &Path) -> Result<()> {
        let curl = Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--retry",
                "3",
                "--connect-timeout",
                "15",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(destination)
            .arg(url)
            .status();

        match curl {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => bail!("curl failed to download {url} with {status}"),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to start curl"),
        }

        let wget = Command::new("wget")
            .args(["--tries=3", "--timeout=15", "--quiet", "--output-document"])
            .arg(destination)
            .arg(url)
            .status();

        match wget {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => bail!("wget failed to download {url} with {status}"),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                bail!("curl or wget is required to update T4E")
            }
            Err(error) => Err(error).context("failed to start wget"),
        }
    }
}

fn execute_update<D, F>(
    config: &UpdateConfig,
    downloader: &D,
    request: UpdateRequest,
    mut progress: F,
) -> Result<UpdateOutcome>
where
    D: Downloader,
    F: FnMut(&str),
{
    let temporary = TemporaryDirectory::new("t4e-update")?;
    let explicitly_requested = request.version.is_some();
    let target_version = if let Some(version) = request.version.as_deref() {
        parse_version(version).context("invalid requested T4E version")?
    } else {
        progress("Checking the latest T4E release...");
        fetch_latest_version(config, downloader, temporary.path())?
    };

    if target_version == config.current_version {
        return Ok(UpdateOutcome::UpToDate {
            version: target_version,
        });
    }
    if !explicitly_requested && target_version < config.current_version {
        return Ok(UpdateOutcome::CurrentIsNewer {
            current: config.current_version.clone(),
            latest: target_version,
        });
    }
    if request.check_only {
        return Ok(UpdateOutcome::Available {
            current: config.current_version.clone(),
            target: target_version,
        });
    }

    let package = format!("t4e-{target_version}-{}", config.asset_label);
    let archive_name = format!("{package}.tar.gz");
    let release_base = format!(
        "{}/{}/releases/download/v{}",
        config.download_base, config.repository, target_version
    );
    let archive_path = temporary.path().join(&archive_name);
    let checksum_path = temporary.path().join(format!("{archive_name}.sha256"));

    progress(&format!(
        "Downloading T4E {target_version} for {}...",
        config.asset_label
    ));
    downloader.download(&format!("{release_base}/{archive_name}"), &archive_path)?;
    downloader.download(
        &format!("{release_base}/{archive_name}.sha256"),
        &checksum_path,
    )?;

    verify_download(&archive_path, &checksum_path, &archive_name)?;
    let downloaded_executable = extract_executable(&archive_path, temporary.path(), &package)?;
    replace_executable(&downloaded_executable, &config.executable)?;

    Ok(UpdateOutcome::Updated {
        previous: config.current_version.clone(),
        installed: target_version,
        executable: config.executable.clone(),
    })
}

fn fetch_latest_version<D: Downloader>(
    config: &UpdateConfig,
    downloader: &D,
    temporary_directory: &Path,
) -> Result<Version> {
    #[derive(Deserialize)]
    struct LatestRelease {
        tag_name: String,
    }

    let metadata_path = temporary_directory.join("latest-release.json");
    let url = format!(
        "{}/repos/{}/releases/latest",
        config.api_base, config.repository
    );
    downloader.download(&url, &metadata_path)?;
    let metadata = fs::read(&metadata_path).context("failed to read latest release metadata")?;
    let release: LatestRelease = serde_json::from_slice(&metadata)
        .context("latest release metadata was not valid GitHub JSON")?;
    parse_version(&release.tag_name).context("latest release has an invalid version tag")
}

fn parse_version(version: &str) -> Result<Version> {
    Version::parse(version.trim().strip_prefix('v').unwrap_or(version.trim()))
        .context("expected a semantic version such as 0.4.0")
}

fn release_asset_label(os: &str, architecture: &str) -> Result<String> {
    let label = match (os, architecture) {
        ("linux", "x86_64") => "linux-x86_64-musl",
        ("linux", "x86" | "i386" | "i486" | "i586" | "i686") => "linux-i686-musl",
        ("linux", "aarch64") => "linux-aarch64-musl",
        ("macos", "aarch64") => "macos-arm64",
        _ => bail!("T4E self-update does not support {os}/{architecture}"),
    };
    Ok(label.to_string())
}

fn verify_download(archive: &Path, checksum_file: &Path, archive_name: &str) -> Result<()> {
    let checksum = fs::read_to_string(checksum_file)
        .with_context(|| format!("failed to read {}", checksum_file.display()))?;
    let expected = parse_release_checksum(&checksum, archive_name)?;
    let actual = sha256_file(archive)?;
    if !actual.eq_ignore_ascii_case(&expected) {
        bail!("release checksum verification failed; the current T4E binary was not changed");
    }
    Ok(())
}

fn parse_release_checksum(contents: &str, archive_name: &str) -> Result<String> {
    let mut fields = contents
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let checksum = fields.next().unwrap_or_default();
    let filename = fields.next().unwrap_or_default().trim_start_matches('*');
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("release checksum is malformed; refusing to update");
    }
    if filename != archive_name {
        bail!("release checksum does not name {archive_name}; refusing to update");
    }
    Ok(checksum.to_ascii_lowercase())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn extract_executable(archive: &Path, destination: &Path, package: &str) -> Result<PathBuf> {
    let listing = Command::new("tar")
        .arg("-tzf")
        .arg(archive)
        .output()
        .context("tar is required to unpack the T4E update")?;
    if !listing.status.success() {
        bail!(
            "could not inspect the T4E release archive: {}",
            String::from_utf8_lossy(&listing.stderr).trim()
        );
    }
    let listing = String::from_utf8(listing.stdout)
        .context("the T4E release archive contains non-UTF-8 paths")?;
    validate_archive_listing(&listing, package)?;

    let relative_executable = format!("{package}/t4e");
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(destination)
        .arg(&relative_executable)
        .status()
        .context("tar is required to unpack the T4E update")?;
    if !status.success() {
        bail!("could not extract the T4E executable from the release archive");
    }

    let executable = destination.join(relative_executable);
    let metadata = fs::symlink_metadata(&executable)
        .context("the T4E release archive did not contain an executable")?;
    if !metadata.file_type().is_file() {
        bail!("the T4E release executable is not a regular file");
    }
    Ok(executable)
}

fn validate_archive_listing(listing: &str, package: &str) -> Result<()> {
    let mut executable_entries = 0;
    let mut entry_count = 0;
    for entry in listing.lines().filter(|entry| !entry.trim().is_empty()) {
        entry_count += 1;
        let path = Path::new(entry.trim_end_matches('/'));
        let mut components = path.components();
        if components.next() != Some(Component::Normal(package.as_ref()))
            || components.any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("release archive has an unexpected path: {entry}");
        }
        if entry.trim_end_matches('/') == format!("{package}/t4e") {
            executable_entries += 1;
        }
    }
    if entry_count == 0 || executable_entries != 1 {
        bail!("release archive must contain exactly one {package}/t4e executable");
    }
    Ok(())
}

fn replace_executable(source: &Path, target: &Path) -> Result<()> {
    let parent = target
        .parent()
        .context("the running T4E executable has no parent directory")?;
    let mut staged_update = None;
    for attempt in 0..100_u32 {
        let candidate = parent.join(format!(".t4e-update-{}-{attempt}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                staged_update = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("cannot write updates beside {}", target.display()));
            }
        }
    }
    let (temporary_target, mut staged_file) =
        staged_update.context("could not allocate a temporary update file")?;
    let result = (|| -> Result<()> {
        let mut source_file = File::open(source).context("failed to open the downloaded update")?;
        std::io::copy(&mut source_file, &mut staged_file)
            .context("failed to stage the T4E update")?;
        #[cfg(unix)]
        staged_file
            .set_permissions(fs::Permissions::from_mode(0o755))
            .context("failed to make the T4E update executable")?;
        staged_file
            .sync_all()
            .context("failed to persist the staged T4E update")?;
        drop(staged_file);
        fs::rename(&temporary_target, target).with_context(|| {
            format!(
                "failed to replace the T4E executable at {}",
                target.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_target);
    }
    result
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new(prefix: &str) -> Result<Self> {
        let base = env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_nanos();
        for attempt in 0..100_u32 {
            let path = base.join(format!("{prefix}-{}-{nonce}-{attempt}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("could not create an update directory"),
            }
        }
        bail!("could not allocate an update directory")
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("t4e-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).expect("test directory");
        path
    }

    struct FixtureDownloader {
        directory: PathBuf,
    }

    impl Downloader for FixtureDownloader {
        fn download(&self, url: &str, destination: &Path) -> Result<()> {
            let filename = url.rsplit('/').next().context("download URL filename")?;
            fs::copy(self.directory.join(filename), destination)
                .with_context(|| format!("missing fixture for {url}"))?;
            Ok(())
        }
    }

    struct RejectDownloader;

    impl Downloader for RejectDownloader {
        fn download(&self, url: &str, _destination: &Path) -> Result<()> {
            bail!("unexpected download: {url}")
        }
    }

    fn make_release_fixture(directory: &Path, version: &str, asset_label: &str) -> String {
        let package = format!("t4e-{version}-{asset_label}");
        let package_directory = directory.join(&package);
        fs::create_dir(&package_directory).expect("package directory");
        fs::write(package_directory.join("t4e"), b"new release binary")
            .expect("fixture executable");
        let archive_name = format!("{package}.tar.gz");
        let archive = directory.join(&archive_name);
        let status = Command::new("tar")
            .arg("-C")
            .arg(directory)
            .arg("-czf")
            .arg(&archive)
            .arg(&package)
            .status()
            .expect("start tar");
        assert!(status.success(), "fixture archive creation failed");
        let checksum = sha256_file(&archive).expect("fixture checksum");
        fs::write(
            directory.join(format!("{archive_name}.sha256")),
            format!("{checksum}  {archive_name}\n"),
        )
        .expect("checksum fixture");
        archive_name
    }

    fn test_config(directory: &Path) -> UpdateConfig {
        let executable = directory.join("installed-t4e");
        fs::write(&executable, b"old release binary").expect("installed fixture");
        UpdateConfig {
            repository: "example/t4e".to_string(),
            api_base: "https://example.invalid/api".to_string(),
            download_base: "https://example.invalid".to_string(),
            current_version: Version::parse("1.0.0").expect("current version"),
            executable,
            asset_label: "linux-x86_64-musl".to_string(),
        }
    }

    #[test]
    fn maps_supported_release_assets() {
        assert_eq!(
            release_asset_label("linux", "x86_64").expect("linux x86_64"),
            "linux-x86_64-musl"
        );
        assert_eq!(
            release_asset_label("linux", "x86").expect("linux x86"),
            "linux-i686-musl"
        );
        assert_eq!(
            release_asset_label("linux", "aarch64").expect("linux arm64"),
            "linux-aarch64-musl"
        );
        assert_eq!(
            release_asset_label("macos", "aarch64").expect("macOS arm64"),
            "macos-arm64"
        );
        assert!(release_asset_label("macos", "x86_64").is_err());
    }

    #[test]
    fn parses_only_the_checksum_for_the_expected_archive() {
        let expected = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(
            parse_release_checksum(
                &format!("{expected}  t4e-1.2.3-linux-x86_64-musl.tar.gz\n"),
                "t4e-1.2.3-linux-x86_64-musl.tar.gz"
            )
            .expect("valid checksum"),
            expected
        );
        assert!(
            parse_release_checksum(
                &format!("{expected}  another-file.tar.gz\n"),
                "t4e-1.2.3-linux-x86_64-musl.tar.gz"
            )
            .is_err()
        );
    }

    #[test]
    fn atomically_replaces_the_existing_executable() {
        let directory = test_dir("atomic-update");
        let source = directory.join("downloaded-t4e");
        let target = directory.join("t4e");
        fs::write(&source, b"new binary").expect("write source");
        fs::write(&target, b"old binary").expect("write target");

        replace_executable(&source, &target).expect("replace executable");

        assert_eq!(fs::read(&target).expect("read target"), b"new binary");
        assert_eq!(
            fs::metadata(&target)
                .expect("target metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert!(
            fs::read_dir(&directory)
                .expect("directory readable")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".t4e-update-"))
        );

        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn installs_a_verified_release_and_replaces_the_running_binary_path() {
        let directory = test_dir("verified-update");
        make_release_fixture(&directory, "1.2.3", "linux-x86_64-musl");
        let config = test_config(&directory);
        let downloader = FixtureDownloader {
            directory: directory.clone(),
        };

        let outcome = execute_update(
            &config,
            &downloader,
            UpdateRequest {
                check_only: false,
                version: Some("1.2.3".to_string()),
            },
            |_| {},
        )
        .expect("verified update succeeds");

        assert_eq!(
            fs::read(&config.executable).expect("updated binary"),
            b"new release binary"
        );
        assert_eq!(
            outcome,
            UpdateOutcome::Updated {
                previous: Version::parse("1.0.0").expect("previous version"),
                installed: Version::parse("1.2.3").expect("installed version"),
                executable: config.executable.clone(),
            }
        );

        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn checksum_failure_preserves_the_installed_binary() {
        let directory = test_dir("rejected-update");
        let archive_name = make_release_fixture(&directory, "1.2.3", "linux-x86_64-musl");
        fs::write(
            directory.join(format!("{archive_name}.sha256")),
            format!("{}  {archive_name}\n", "0".repeat(64)),
        )
        .expect("bad checksum fixture");
        let config = test_config(&directory);
        let downloader = FixtureDownloader {
            directory: directory.clone(),
        };

        let error = execute_update(
            &config,
            &downloader,
            UpdateRequest {
                check_only: false,
                version: Some("1.2.3".to_string()),
            },
            |_| {},
        )
        .expect_err("bad checksum rejected");

        assert!(error.to_string().contains("checksum verification failed"));
        assert_eq!(
            fs::read(&config.executable).expect("original binary"),
            b"old release binary"
        );

        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn check_only_with_a_version_does_not_download_release_assets() {
        let directory = test_dir("check-update");
        let config = test_config(&directory);

        let outcome = execute_update(
            &config,
            &RejectDownloader,
            UpdateRequest {
                check_only: true,
                version: Some("1.2.3".to_string()),
            },
            |_| {},
        )
        .expect("check succeeds without a download");

        assert_eq!(
            outcome,
            UpdateOutcome::Available {
                current: Version::parse("1.0.0").expect("current version"),
                target: Version::parse("1.2.3").expect("target version"),
            }
        );
        assert_eq!(
            fs::read(&config.executable).expect("original binary"),
            b"old release binary"
        );

        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn rejects_archive_paths_outside_the_versioned_package() {
        assert!(
            validate_archive_listing(
                "t4e-1.2.3-linux-x86_64-musl/\nt4e-1.2.3-linux-x86_64-musl/t4e\n../escape\n",
                "t4e-1.2.3-linux-x86_64-musl"
            )
            .is_err()
        );
    }
}
