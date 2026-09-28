//! Git worktree isolation, one worktree per session.
//!
//! We shell out to the `git` CLI (the ecosystem norm — t3code, container-use, opencode plugins —
//! and more capable here than git libraries). The session's `worktree_path` is the checkout root;
//! its `cwd` mirrors the originally selected repository subdirectory inside that checkout, so the
//! provider runs against an isolated branch without losing project-local configuration.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
    claim: DirectoryClaim,
}

impl Worktree {
    /// Whether the checkout root is still the exact directory atomically claimed by this creator.
    /// Git metadata alone is insufficient: a hook can copy `.git` into a replacement directory.
    pub fn still_owns_path(&self) -> io::Result<bool> {
        self.claim.still_owns_path()
    }

    /// Filesystem identity captured for this checkout at creation time.
    pub fn directory_identity(&self) -> &DirectoryIdentity {
        &self.claim.identity
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRegistration {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// The checkout from which a new session worktree should branch.
///
/// Both variants are resolved from refs already present in the local repository. Resolution never
/// fetches, guesses a branch name, or silently substitutes a different baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeBaseline {
    /// The commit checked out by the source worktree's `HEAD`.
    Current,
    /// The commit targeted by the local symbolic ref `refs/remotes/origin/HEAD`.
    OriginDefault,
}

/// An immutable, local-only worktree baseline ready to pass to `git worktree add`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedWorktreeBaseline {
    pub kind: WorktreeBaseline,
    #[serde(rename = "ref")]
    pub reference: String,
    pub sha: String,
    pub display: String,
}

const WORKTREE_DIR: &str = ".codetwo-worktrees";
const WORKTREE_SETTINGS_FILE: &str = "worktree-settings.json";
const ORIGIN_HEAD: &str = "refs/remotes/origin/HEAD";

pub(crate) fn session_checkout_name(repo_root: &Path) -> &OsStr {
    repo_root
        .file_name()
        .filter(|name| name.to_str().is_some())
        .unwrap_or_else(|| OsStr::new("repo"))
}

fn is_canonical_session_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn remove_empty_checkout_in(path: &Path, container: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    if parent != container && parent.parent() == Some(container) {
        let _ = std::fs::remove_dir(parent);
    }
    let _ = std::fs::remove_dir(container);
}

fn remove_empty_session_parent(path: &Path, session_id: &str) {
    let Some(parent) = path.parent() else {
        return;
    };
    if parent.file_name() != Some(OsStr::new(session_id)) {
        return;
    }
    let Some(container) = parent.parent() else {
        return;
    };
    let _ = std::fs::remove_dir(parent);
    if container.file_name() == Some(OsStr::new(WORKTREE_DIR)) {
        let _ = std::fs::remove_dir(container);
    }
}

fn default_auto_delete_limit() -> usize {
    15
}

/// Global policy for session worktrees. Defaults preserve the existing project-adjacent layout
/// and explicit cleanup behavior until the user opts into automation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default)]
    pub fetch_upstream: bool,
    #[serde(default)]
    pub auto_delete: bool,
    #[serde(default = "default_auto_delete_limit")]
    pub auto_delete_limit: usize,
}

impl Default for WorktreeSettings {
    fn default() -> Self {
        Self {
            root: None,
            fetch_upstream: false,
            auto_delete: false,
            auto_delete_limit: default_auto_delete_limit(),
        }
    }
}

impl WorktreeSettings {
    fn normalized(mut self) -> io::Result<Self> {
        self.root = self
            .root
            .take()
            .map(|root| root.trim().to_string())
            .filter(|root| !root.is_empty());
        if let Some(root) = self.root.as_deref() {
            let path = Path::new(root);
            if !path.is_absolute() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "worktree root must be an absolute path",
                ));
            }
            if path.exists() && !path.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "worktree root must be a directory",
                ));
            }
        }
        self.auto_delete_limit = self.auto_delete_limit.clamp(1, 1000);
        Ok(self)
    }
}

pub fn load_settings(data_dir: &Path) -> io::Result<WorktreeSettings> {
    let path = data_dir.join(WORKTREE_SETTINGS_FILE);
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str::<WorktreeSettings>(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
            .normalized(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(WorktreeSettings::default()),
        Err(error) => Err(error),
    }
}

pub fn save_settings(data_dir: &Path, settings: WorktreeSettings) -> io::Result<WorktreeSettings> {
    let settings = settings.normalized()?;
    fs::create_dir_all(data_dir)?;
    let path = data_dir.join(WORKTREE_SETTINGS_FILE);
    let temporary = data_dir.join(format!(
        ".{WORKTREE_SETTINGS_FILE}.{}.tmp",
        std::process::id()
    ));
    let contents = serde_json::to_string_pretty(&settings)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(&temporary, format!("{contents}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&temporary, path)?;
    Ok(settings)
}

fn git_command(repo: &Path) -> Command {
    let mut command = Command::new("git");
    // A rev-parse or checkout can otherwise contact a promisor remote when a partial clone is
    // missing an object. Baseline discovery and creation are a strictly local operation.
    command.env("GIT_NO_LAZY_FETCH", "1").arg("-C").arg(repo);
    command
}

fn git_dir_command(common_dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .env("GIT_NO_LAZY_FETCH", "1")
        .arg("--git-dir")
        .arg(common_dir);
    command
}

async fn run_git_output(repo: &Path, args: &[&OsStr]) -> io::Result<Output> {
    git_command(repo).args(args).output().await
}

async fn run_git_dir_output(common_dir: &Path, args: &[&OsStr]) -> io::Result<Output> {
    git_dir_command(common_dir).args(args).output().await
}

async fn run_git_dir(common_dir: &Path, args: &[&OsStr]) -> io::Result<String> {
    let out = run_git_dir_output(common_dir, args).await?;
    if !out.status.success() {
        return Err(git_failure(&out));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn git_failure(out: &Output) -> io::Error {
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    io::Error::other(format!("git failed: {stderr}"))
}

async fn run_git(repo: &Path, args: &[&OsStr]) -> io::Result<String> {
    let out = run_git_output(repo, args).await?;
    if !out.status.success() {
        return Err(git_failure(&out));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Refresh the repository's configured remotes before resolving a worktree baseline.
pub async fn fetch_upstream(repo: &Path) -> io::Result<()> {
    run_git(repo, &[OsStr::new("fetch"), OsStr::new("--prune")])
        .await
        .map(|_| ())
}

fn validate_resolved_sha(sha: &str) -> io::Result<()> {
    if matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "worktree baseline SHA must be a full SHA-1 or SHA-256 object id",
        ))
    }
}

async fn resolve_commit(repo: &Path, reference: &str) -> io::Result<String> {
    let revision = OsString::from(format!("{reference}^{{commit}}"));
    let sha = run_git(
        repo,
        &[
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            revision.as_os_str(),
        ],
    )
    .await?;
    let sha = sha.trim().to_string();
    validate_resolved_sha(&sha)?;
    Ok(sha)
}

/// Exact commit currently checked out at `path`.
pub async fn head_commit(path: &Path) -> io::Result<String> {
    resolve_commit(path, "HEAD").await
}

fn display_ref(reference: &str) -> &str {
    reference
        .strip_prefix("refs/remotes/")
        .or_else(|| reference.strip_prefix("refs/heads/"))
        .unwrap_or(reference)
}

fn baseline_display(reference: &str, sha: &str) -> String {
    let short_sha = &sha[..sha.len().min(8)];
    format!("{} @ {short_sha}", display_ref(reference))
}

/// Resolve a worktree baseline using only refs and objects already available in `repo`.
pub async fn resolve_baseline(
    repo: &Path,
    kind: WorktreeBaseline,
) -> io::Result<ResolvedWorktreeBaseline> {
    match kind {
        WorktreeBaseline::Current => {
            let sha = resolve_commit(repo, "HEAD").await?;
            let symbolic = run_git_output(
                repo,
                &[
                    OsStr::new("symbolic-ref"),
                    OsStr::new("--quiet"),
                    OsStr::new("--short"),
                    OsStr::new("HEAD"),
                ],
            )
            .await?;
            let reference = if symbolic.status.success() {
                let branch = String::from_utf8_lossy(&symbolic.stdout).trim().to_string();
                if branch.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "current HEAD symbolic ref resolved to an empty branch",
                    ));
                }
                branch
            } else if symbolic.status.code() == Some(1) {
                // `git symbolic-ref --quiet` uses status 1 when HEAD is detached.
                "HEAD".to_string()
            } else {
                return Err(git_failure(&symbolic));
            };
            let display = baseline_display(&reference, &sha);
            Ok(ResolvedWorktreeBaseline {
                kind,
                reference,
                sha,
                display,
            })
        }
        WorktreeBaseline::OriginDefault => {
            let symbolic = run_git_output(
                repo,
                &[
                    OsStr::new("symbolic-ref"),
                    OsStr::new("--quiet"),
                    OsStr::new(ORIGIN_HEAD),
                ],
            )
            .await?;
            if !symbolic.status.success() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "origin default baseline is unavailable: {ORIGIN_HEAD} is missing or is not a symbolic ref"
                    ),
                ));
            }

            let reference = String::from_utf8_lossy(&symbolic.stdout).trim().to_string();
            let origin_branch = reference
                .strip_prefix("refs/remotes/origin/")
                .filter(|branch| !branch.is_empty() && *branch != "HEAD")
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "origin default baseline is invalid: {ORIGIN_HEAD} points outside refs/remotes/origin"
                        ),
                    )
                })?;

            let sha = resolve_commit(repo, &reference).await.map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "origin default baseline is invalid: {ORIGIN_HEAD} points to {reference}, but that local ref does not resolve to a commit"
                    ),
                )
            })?;
            let display = baseline_display(&format!("origin/{origin_branch}"), &sha);
            Ok(ResolvedWorktreeBaseline {
                kind,
                reference,
                sha,
                display,
            })
        }
    }
}

fn add_from_sha_args<'a>(path: &'a Path, sha: &'a str) -> [&'a OsStr; 5] {
    [
        OsStr::new("worktree"),
        OsStr::new("add"),
        OsStr::new("--detach"),
        path.as_os_str(),
        OsStr::new(sha),
    ]
}

fn branch_ref(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

async fn validate_new_branch(repo: &Path, branch: &str) -> io::Result<String> {
    let validated = run_git_output(
        repo,
        &[
            OsStr::new("check-ref-format"),
            OsStr::new("--branch"),
            OsStr::new(branch),
        ],
    )
    .await?;
    if !validated.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid worktree branch {branch:?}: {}",
                git_failure(&validated)
            ),
        ));
    }
    Ok(branch_ref(branch))
}

async fn branch_target(repo: &Path, reference: &str) -> io::Result<Option<String>> {
    let exists = run_git_output(
        repo,
        &[
            OsStr::new("show-ref"),
            OsStr::new("--verify"),
            OsStr::new("--quiet"),
            OsStr::new(reference),
        ],
    )
    .await?;
    if !exists.status.success() {
        return if exists.status.code() == Some(1) {
            Ok(None)
        } else {
            Err(git_failure(&exists))
        };
    }
    let sha = run_git(
        repo,
        &[
            OsStr::new("show-ref"),
            OsStr::new("--verify"),
            OsStr::new("--hash"),
            OsStr::new(reference),
        ],
    )
    .await?;
    let sha = sha.trim().to_string();
    validate_resolved_sha(&sha)?;
    Ok(Some(sha))
}

/// Stable filesystem identity for a directory.
///
/// This is persisted with a session so later operations can reject a path that was removed and
/// replaced, even when the replacement contains plausible copied Git metadata. The representation
/// deliberately uses only APIs available on the workspace's Rust 1.82 MSRV.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DirectoryIdentity {
    Unix {
        device: u64,
        inode: u64,
        /// Distinguishes recycled inode numbers when the filesystem exposes a birth time.
        /// Older records and filesystems without birth times retain device/inode validation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        birth_time_ns: Option<u64>,
    },
    Windows {
        volume_serial_number: u32,
        file_index: u64,
    },
}

impl DirectoryIdentity {
    /// Capture the identity of a real directory without following a leaf symlink/reparse point.
    pub fn capture(path: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "directory identity requires a real directory: {}",
                        path.display()
                    ),
                ));
            }
            Ok(Self::Unix {
                device: metadata.dev(),
                inode: metadata.ino(),
                birth_time_ns: metadata
                    .created()
                    .ok()
                    .and_then(|created| created.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|elapsed| u64::try_from(elapsed.as_nanos()).ok()),
            })
        }
        #[cfg(windows)]
        {
            windows_directory_identity(path)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "stable directory identity is unsupported on this platform",
            ))
        }
    }

    /// Whether `path` currently names this exact directory.
    pub fn matches_path(&self, path: &Path) -> io::Result<bool> {
        let actual = match Self::capture(path) {
            Ok(identity) => identity,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(match (self, &actual) {
            (
                Self::Unix {
                    device,
                    inode,
                    birth_time_ns,
                },
                Self::Unix {
                    device: actual_device,
                    inode: actual_inode,
                    birth_time_ns: actual_birth,
                },
            ) => {
                device == actual_device
                    && inode == actual_inode
                    && birth_time_ns.map_or(true, |expected| *actual_birth == Some(expected))
            }
            _ => *self == actual,
        })
    }
}

#[cfg(windows)]
fn windows_directory_identity(path: &Path) -> io::Result<DirectoryIdentity> {
    use std::ffi::c_void;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    #[repr(C)]
    struct FileTime {
        low_date_time: u32,
        high_date_time: u32,
    }

    #[repr(C)]
    struct ByHandleFileInformation {
        file_attributes: u32,
        creation_time: FileTime,
        last_access_time: FileTime,
        last_write_time: FileTime,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(
            file: *mut c_void,
            information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    // Backup semantics is required to open a directory. OPEN_REPARSE_POINT plus the attribute
    // check below ensures a junction/symlink is identified as the leaf object and rejected rather
    // than followed. Sharing deletion is safe here because this is a non-destructive observation.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let mut information = std::mem::MaybeUninit::<ByHandleFileInformation>::uninit();
    // SAFETY: `file` owns a valid handle for the duration of the call and the output points to
    // sufficient writable storage. A nonzero result initializes the entire documented structure.
    let succeeded = unsafe {
        GetFileInformationByHandle(file.as_raw_handle().cast(), information.as_mut_ptr())
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: established by the successful Win32 call above.
    let information = unsafe { information.assume_init() };
    if information.file_attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || information.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "directory identity requires a real directory: {}",
                path.display()
            ),
        ));
    }
    Ok(DirectoryIdentity::Windows {
        volume_serial_number: information.volume_serial_number,
        file_index: u64::from(information.file_index_high) << 32
            | u64::from(information.file_index_low),
    })
}

#[derive(Debug, Clone)]
struct DirectoryClaim {
    path: PathBuf,
    identity: DirectoryIdentity,
}

impl DirectoryClaim {
    fn create(path: &Path) -> io::Result<Self> {
        // `create_dir` is the ownership boundary: unlike `try_exists`, it also rejects dangling
        // symlinks and cannot race another creator into silently sharing the destination.
        std::fs::create_dir(path)?;
        let identity = DirectoryIdentity::capture(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            identity,
        })
    }

    fn still_owns_path(&self) -> io::Result<bool> {
        self.identity.matches_path(&self.path)
    }

    fn quarantine(&self) -> io::Result<PathBuf> {
        if !self.still_owns_path()? {
            return Err(io::Error::other(format!(
                "refusing destructive cleanup because worktree path identity changed: {}",
                self.path.display()
            )));
        }
        self.refuse_non_atomic_quarantine()
    }

    #[cfg(all(test, unix))]
    fn quarantine_after_identity_check_for_test(
        &self,
        after_identity_check: impl FnOnce(),
    ) -> io::Result<PathBuf> {
        if !self.still_owns_path()? {
            return Err(io::Error::other(format!(
                "refusing destructive cleanup because worktree path identity changed: {}",
                self.path.display()
            )));
        }
        // Deterministically models a replacement in the old check-then-rename window.
        after_identity_check();
        self.refuse_non_atomic_quarantine()
    }

    fn refuse_non_atomic_quarantine(&self) -> io::Result<PathBuf> {
        // POSIX has no compare-identity-and-rename primitive: an open directory fd pins the object
        // but `renameat(2)` still resolves the source leaf by name. Documented Win32 directory
        // creation likewise does not atomically return the handle needed to bind all later cleanup
        // to the created object. A check followed by any path-based rename can therefore move an
        // attacker's replacement directory. Preserve the path, registration, and ref instead.
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "safe worktree quarantine is unavailable because this platform has no atomic compare-identity-and-rename operation; retained claimed path for manual cleanup: {}",
                self.path.display()
            ),
        ))
    }
}

async fn rollback_detached_add(claim: &DirectoryClaim, cause: io::Error) -> io::Error {
    let mut errors = Vec::new();
    let quarantine = match claim.quarantine() {
        Ok(path) => path,
        Err(error) => {
            errors.push(error.to_string());
            return io::Error::new(
                cause.kind(),
                format!(
                    "{cause}; additionally couldn't safely roll back worktree creation: {}",
                    errors.join("; ")
                ),
            );
        }
    };
    errors.push(format!(
        "preserved claimed checkout at {} and any stale Git registration for concurrency-safe manual cleanup",
        quarantine.display(),
    ));
    if errors.is_empty() {
        cause
    } else {
        io::Error::new(
            cause.kind(),
            format!(
                "{cause}; additionally couldn't fully roll back worktree creation: {}",
                errors.join("; ")
            ),
        )
    }
}

/// Undo a worktree creation that never became durable application state.
///
/// The caller must have established that both `path` and `branch` were absent before creation.
/// The path, Git registration, and branch are deliberately retained for manual cleanup. Neither Git
/// nor the supported filesystems offer an atomic compare-directory-identity-and-rename primitive;
/// a path-based move/remove could act on a replacement directory, while ref cleanup could break a
/// checkout attached concurrently by another process.
pub async fn rollback_created(
    repo: &Path,
    worktree: &Worktree,
    expected_sha: &str,
) -> io::Result<()> {
    validate_resolved_sha(expected_sha)?;
    let branch = &worktree.branch;
    let reference = validate_new_branch(repo, branch).await?;
    let mut errors = Vec::new();

    let quarantine = worktree.claim.quarantine()?;
    errors.push(format!(
        "preserved claimed checkout at {} and its Git registration for concurrency-safe manual cleanup",
        quarantine.display(),
    ));
    let branch_in_use = match registrations(repo).await {
        Ok(worktrees) => worktrees
            .iter()
            .any(|entry| entry.branch.as_deref() == Some(reference.as_str())),
        Err(error) => {
            errors.push(format!(
                "couldn't inspect whether created branch {branch} is registered: {error}"
            ));
            false
        }
    };
    if branch_in_use {
        errors.push(format!(
            "left created branch {branch} intact because a worktree registry entry still uses it"
        ));
    } else {
        match branch_target(repo, &reference).await {
            Ok(Some(current)) if current == expected_sha => errors.push(format!(
                "left created branch {branch} at {expected_sha} for concurrency-safe manual cleanup"
            )),
            Ok(Some(current)) => errors.push(format!(
                "created branch {branch} changed during rollback (expected {expected_sha}, found {current}); left it untouched"
            )),
            Ok(None) => {}
            Err(error) => errors.push(format!("couldn't inspect created branch {branch}: {error}")),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(errors.join("; ")))
    }
}

async fn rollback_error(
    repo: &Path,
    worktree: &Worktree,
    expected_sha: &str,
    cause: io::Error,
) -> io::Error {
    match rollback_created(repo, worktree, expected_sha).await {
        Ok(()) => cause,
        Err(cleanup) => io::Error::new(
            cause.kind(),
            format!("{cause}; additionally couldn't fully roll back worktree creation: {cleanup}"),
        ),
    }
}

/// `git worktree add -b <branch> <path> <sha>` — create a branch from an already resolved commit.
pub async fn add_from_sha(
    repo: &Path,
    path: &Path,
    branch: &str,
    sha: &str,
) -> io::Result<Worktree> {
    validate_resolved_sha(sha)?;
    let reference = validate_new_branch(repo, branch).await?;
    let claim = DirectoryClaim::create(path).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("worktree path already exists: {}", path.display()),
            )
        } else {
            error
        }
    })?;
    if let Err(cause) = run_git(repo, &add_from_sha_args(path, sha)).await {
        return Err(rollback_detached_add(&claim, cause).await);
    }

    // Claim the branch atomically only after the exact detached commit is checked out. A zero old
    // value means an independently-created ref can never be mistaken for ours and deleted later.
    let zero = "0".repeat(sha.len());
    if let Err(cause) = run_git(
        repo,
        &[
            OsStr::new("update-ref"),
            OsStr::new(&reference),
            OsStr::new(sha),
            OsStr::new(&zero),
        ],
    )
    .await
    {
        return Err(rollback_detached_add(&claim, cause).await);
    }
    let worktree = Worktree {
        path: path.to_path_buf(),
        branch: branch.to_string(),
        claim,
    };
    if let Err(cause) = run_git(
        path,
        &[
            OsStr::new("checkout"),
            OsStr::new("--quiet"),
            OsStr::new(branch),
        ],
    )
    .await
    {
        return Err(rollback_error(repo, &worktree, sha, cause).await);
    }
    let checked_out = match resolve_commit(path, "HEAD").await {
        Ok(sha) => sha,
        Err(cause) => return Err(rollback_error(repo, &worktree, sha, cause).await),
    };
    if checked_out != sha {
        let cause = io::Error::new(
            io::ErrorKind::InvalidData,
            format!("worktree checked out {checked_out}, expected baseline {sha}"),
        );
        return Err(rollback_error(repo, &worktree, sha, cause).await);
    }
    Ok(worktree)
}

/// Create a fresh branch at the current checkout's resolved commit.
pub async fn add(repo: &Path, path: &Path, branch: &str) -> io::Result<Worktree> {
    let baseline = resolve_baseline(repo, WorktreeBaseline::Current).await?;
    add_from_sha(repo, path, branch, &baseline.sha).await
}

/// Resolve the checkout root even when the user selected a directory inside the repository.
pub async fn repo_root(path: &Path) -> io::Result<PathBuf> {
    let root = run_git(
        path,
        &[OsStr::new("rev-parse"), OsStr::new("--show-toplevel")],
    )
    .await?;
    std::fs::canonicalize(root.trim())
}

/// Canonical common Git directory shared by every linked worktree in one repository.
pub async fn common_dir(path: &Path) -> io::Result<PathBuf> {
    let raw = run_git(
        path,
        &[OsStr::new("rev-parse"), OsStr::new("--git-common-dir")],
    )
    .await?;
    let raw = PathBuf::from(raw.trim());
    let raw = if raw.is_absolute() {
        raw
    } else {
        path.join(raw)
    };
    std::fs::canonicalize(raw)
}

/// Canonical per-worktree Git administrative directory (`.git/worktrees/<name>` for a linked
/// checkout). Unlike the common directory, this distinguishes two worktrees in the same repo.
pub async fn git_dir(path: &Path) -> io::Result<PathBuf> {
    let raw = run_git(path, &[OsStr::new("rev-parse"), OsStr::new("--git-dir")]).await?;
    let raw = PathBuf::from(raw.trim());
    let raw = if raw.is_absolute() {
        raw
    } else {
        path.join(raw)
    };
    std::fs::canonicalize(raw)
}

async fn add_for_session_at_root(
    root: &Path,
    session_id: &str,
    baseline: &ResolvedWorktreeBaseline,
    configured_root: Option<&Path>,
) -> io::Result<Worktree> {
    let branch = branch_for_session(session_id)?;
    let safe_id = branch
        .strip_prefix("codetwo/")
        .expect("session branch always has the codetwo prefix");
    let repo_name = session_checkout_name(root);
    let container = session_container_dir_with_root(root, configured_root).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository has no parent directory",
        )
    })?;
    std::fs::create_dir_all(&container)?;
    // Production sessions use canonical UUIDs. Keep the unique id in the parent so Remote sees
    // the stable repository basename at the leaf; retain the flat shape for older helper callers.
    let path = if is_canonical_session_uuid(safe_id) {
        let session_container = container.join(safe_id);
        if let Err(error) = std::fs::create_dir(&session_container) {
            let _ = std::fs::remove_dir(&container);
            return Err(if error.kind() == io::ErrorKind::AlreadyExists {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "worktree session directory already exists: {}",
                        session_container.display()
                    ),
                )
            } else {
                error
            });
        }
        session_container.join(repo_name)
    } else {
        container.join(format!("{}-{safe_id}", repo_name.to_string_lossy()))
    };
    match add_from_sha(root, &path, &branch, &baseline.sha).await {
        Ok(mut worktree) => {
            let canonical = match std::fs::canonicalize(&worktree.path) {
                Ok(path) => path,
                Err(cause) => {
                    return Err(rollback_error(root, &worktree, &baseline.sha, cause).await);
                }
            };
            worktree.path.clone_from(&canonical);
            worktree.claim.path = canonical;
            Ok(worktree)
        }
        Err(error) => {
            // Only removes directories if this failed attempt left them empty.
            remove_empty_checkout_in(&path, &container);
            Err(error)
        }
    }
}

/// Deterministic branch owned by a session worktree.
pub fn branch_for_session(session_id: &str) -> io::Result<String> {
    let safe_id: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if safe_id.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty worktree session id",
        ))
    } else {
        Ok(format!("codetwo/{safe_id}"))
    }
}

/// Create the persistent isolated checkout for one session from an already resolved baseline.
///
/// Worktrees live beside the repository rather than inside it, where Git would make every session
/// appear as an untracked directory. Session ids are generated internally, but sanitize anyway so
/// this helper can never turn an external id into a path or ref traversal.
pub async fn add_for_session_from_baseline(
    repo: &Path,
    session_id: &str,
    baseline: &ResolvedWorktreeBaseline,
) -> io::Result<Worktree> {
    add_for_session_from_baseline_in(repo, session_id, baseline, None).await
}

/// Create a session checkout under a configured global root. A stable repository subdirectory
/// prevents two repositories with the same folder name from sharing a stale-path cleanup scope.
pub async fn add_for_session_from_baseline_in(
    repo: &Path,
    session_id: &str,
    baseline: &ResolvedWorktreeBaseline,
    configured_root: Option<&Path>,
) -> io::Result<Worktree> {
    let root = repo_root(repo).await?;
    add_for_session_at_root(&root, session_id, baseline, configured_root).await
}

/// Create the persistent isolated checkout for one session at the current checkout's `HEAD`.
pub async fn add_for_session(repo: &Path, session_id: &str) -> io::Result<Worktree> {
    let baseline = resolve_baseline(repo, WorktreeBaseline::Current).await?;
    add_for_session_from_baseline(repo, session_id, &baseline).await
}

/// The branch namespace reserved for session worktrees. Discard refuses to touch anything else.
const SESSION_BRANCH_PREFIX: &str = "codetwo/";

/// Receipt of an explicit worktree discard: what actually got removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscardedWorktree {
    /// The checkout directory existed and was removed. `false` means it was already gone and only
    /// the stale registration/branch needed cleanup.
    pub removed_checkout: bool,
    /// The session branch that was deleted, when it existed and no other checkout still used it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_branch: Option<String>,
}

async fn branch_target_in_common_dir(
    common_dir: &Path,
    reference: &str,
) -> io::Result<Option<String>> {
    let out = run_git_dir_output(
        common_dir,
        &[
            OsStr::new("show-ref"),
            OsStr::new("--verify"),
            OsStr::new("--hash"),
            OsStr::new(reference),
        ],
    )
    .await?;
    if !out.status.success() {
        return if out.status.code() == Some(1) {
            Ok(None)
        } else {
            Err(git_failure(&out))
        };
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    validate_resolved_sha(&sha)?;
    Ok(Some(sha))
}

/// Explicit, user-confirmed permanent discard of one session worktree: remove the checkout (even
/// when dirty), drop its Git registration, and delete its `codetwo/…` branch.
///
/// This is the "explicit cleanup flow" that automatic rollback deliberately lacks. It remains
/// destructive-by-consent only:
/// - the branch must be inside the `codetwo/` namespace owned by session worktrees;
/// - when the caller holds a recorded [`DirectoryIdentity`], the directory at `root` must still be
///   that exact filesystem object — a replacement directory fails closed;
/// - the checkout must still be registered to `common_dir` under exactly that branch, so a path
///   Git no longer recognizes as this repository's worktree is never force-removed;
/// - the branch ref is deleted at its observed SHA, and only while no registration still uses it.
///
/// The identity check and the removal are necessarily two steps: no supported platform offers an
/// atomic compare-identity-and-remove primitive. Automatic cleanup refuses on that ground; this
/// function runs only from a user-confirmed discard action, which carries the same authority as
/// the user running `git worktree remove --force` by hand.
pub async fn discard_session_worktree(
    common_dir: &Path,
    root: &Path,
    identity: Option<&DirectoryIdentity>,
    branch: &str,
) -> io::Result<DiscardedWorktree> {
    let suffix = branch.strip_prefix(SESSION_BRANCH_PREFIX).unwrap_or("");
    if suffix.is_empty()
        || !suffix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to discard branch {branch:?}: not a codetwo/ session branch"),
        ));
    }
    let reference = branch_ref(branch);

    let removed_checkout = match std::fs::symlink_metadata(root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // The checkout is already gone. Drop any stale registration so the branch below is no
            // longer considered in use by a phantom checkout.
            run_git_dir(common_dir, &[OsStr::new("worktree"), OsStr::new("prune")]).await?;
            false
        }
        Err(error) => return Err(error),
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(io::Error::other(format!(
                    "refusing to discard {}: not the recorded worktree directory",
                    root.display()
                )));
            }
            if let Some(identity) = identity {
                if !identity.matches_path(root)? {
                    return Err(io::Error::other(format!(
                        "refusing to discard {}: directory identity changed since the worktree was created",
                        root.display()
                    )));
                }
            }
            let canonical = std::fs::canonicalize(root)?;
            let registered = registrations_from_common_dir(common_dir)
                .await?
                .into_iter()
                .find(|registration| {
                    std::fs::canonicalize(&registration.path)
                        .map(|path| path == canonical)
                        .unwrap_or(false)
                });
            let Some(registered) = registered else {
                return Err(io::Error::other(format!(
                    "refusing to discard {}: not registered as a worktree of this repository",
                    root.display()
                )));
            };
            if registered.branch.as_deref() != Some(reference.as_str()) {
                return Err(io::Error::other(format!(
                    "refusing to discard {}: expected branch {reference}, found {}",
                    root.display(),
                    registered.branch.as_deref().unwrap_or("a detached HEAD"),
                )));
            }
            run_git_dir(
                common_dir,
                &[
                    OsStr::new("worktree"),
                    OsStr::new("remove"),
                    OsStr::new("--force"),
                    canonical.as_os_str(),
                ],
            )
            .await?;
            true
        }
    };
    if is_canonical_session_uuid(suffix) {
        remove_empty_session_parent(root, suffix);
    }
    // Existing releases used a flat `<repo>-<session>` checkout even when the session id was a
    // UUID. Continue pruning the default container for those recorded paths.
    if let Some(parent) = root.parent() {
        if parent.file_name() == Some(OsStr::new(WORKTREE_DIR)) {
            let _ = std::fs::remove_dir(parent);
        }
    }

    let branch_still_in_use = registrations_from_common_dir(common_dir)
        .await?
        .iter()
        .any(|registration| registration.branch.as_deref() == Some(reference.as_str()));
    if branch_still_in_use {
        return Ok(DiscardedWorktree {
            removed_checkout,
            deleted_branch: None,
        });
    }
    let deleted_branch = match branch_target_in_common_dir(common_dir, &reference).await? {
        None => None,
        Some(sha) => {
            // Deleting at the observed SHA turns a concurrent ref move into a clean failure
            // instead of silently deleting someone else's new commit.
            run_git_dir(
                common_dir,
                &[
                    OsStr::new("update-ref"),
                    OsStr::new("-d"),
                    OsStr::new(&reference),
                    OsStr::new(&sha),
                ],
            )
            .await?;
            Some(branch.to_string())
        }
    };
    Ok(DiscardedWorktree {
        removed_checkout,
        deleted_branch,
    })
}

/// The shared container directory that holds every session checkout for `repo_root`.
fn stable_path_hash(path: &Path) -> u64 {
    path.as_os_str()
        .to_string_lossy()
        .bytes()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

pub fn session_container_dir_with_root(
    repo_root: &Path,
    configured_root: Option<&Path>,
) -> Option<PathBuf> {
    match configured_root {
        Some(root) => {
            let repo_name = session_checkout_name(repo_root).to_string_lossy();
            Some(root.join(format!("{repo_name}-{:016x}", stable_path_hash(repo_root))))
        }
        None => repo_root.parent().map(|parent| parent.join(WORKTREE_DIR)),
    }
}

pub fn session_container_dir(repo_root: &Path) -> Option<PathBuf> {
    session_container_dir_with_root(repo_root, None)
}

/// Whether a checkout uses either the current `<session UUID>/<repo>` shape or C2's legacy flat
/// shape inside the repository-specific container selected by the caller.
pub(crate) fn is_managed_session_checkout(
    repo_root: &Path,
    container: &Path,
    checkout_path: &Path,
) -> bool {
    let Some(parent) = checkout_path.parent() else {
        return false;
    };
    let repo_name = session_checkout_name(repo_root);

    if parent == container {
        let Some(name) = checkout_path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        let repo_prefix = format!("{}-", repo_name.to_string_lossy());
        return name.starts_with(&repo_prefix) || name.starts_with(".codetwo-rollback-");
    }

    parent.parent() == Some(container)
        && checkout_path.file_name() == Some(repo_name)
        && parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_canonical_session_uuid)
}

/// Test-only raw removal. Production cleanup goes through [`discard_session_worktree`], which
/// re-verifies directory identity and registration first; Git's force removal alone is not
/// path-identity aware.
#[cfg(test)]
pub(crate) async fn remove(repo: &Path, path: &Path) -> io::Result<()> {
    run_git(
        repo,
        &[
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            path.as_os_str(),
        ],
    )
    .await
    .map(|_| ())
}

fn path_from_git_bytes(bytes: &[u8]) -> io::Result<PathBuf> {
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git worktree registry contained an empty path",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        let path = std::str::from_utf8(bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("git worktree path was not valid UTF-8: {error}"),
            )
        })?;
        Ok(PathBuf::from(path))
    }
}

fn parse_registrations(out: &[u8]) -> io::Result<Vec<WorktreeRegistration>> {
    let mut registrations = Vec::new();
    let mut path = None;
    let mut branch = None;
    for field in out.split(|byte| *byte == 0).chain(std::iter::once(&[][..])) {
        if field.is_empty() {
            if let Some(path) = path.take() {
                registrations.push(WorktreeRegistration {
                    path,
                    branch: branch.take(),
                });
            }
        } else if let Some(value) = field.strip_prefix(b"worktree ") {
            if path.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "git worktree registry omitted a record separator",
                ));
            }
            path = Some(path_from_git_bytes(value)?);
        } else if let Some(value) = field.strip_prefix(b"branch ") {
            if path.is_none() || value.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "git worktree registry contained a branch outside a worktree record",
                ));
            }
            branch = Some(
                std::str::from_utf8(value)
                    .map_err(|error| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("git worktree branch was not valid UTF-8: {error}"),
                        )
                    })?
                    .to_string(),
            );
        } else if path.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git worktree registry contained metadata outside a worktree record",
            ));
        }
    }
    Ok(registrations)
}

/// Parse NUL-delimited `git worktree list --porcelain -z` into registered paths and branches.
pub async fn registrations(repo: &Path) -> io::Result<Vec<WorktreeRegistration>> {
    let out = run_git_output(
        repo,
        &[
            OsStr::new("worktree"),
            OsStr::new("list"),
            OsStr::new("--porcelain"),
            OsStr::new("-z"),
        ],
    )
    .await?;
    if !out.status.success() {
        return Err(git_failure(&out));
    }
    parse_registrations(&out.stdout)
}

/// Read the registry directly from a persisted common Git directory, even if the source worktree
/// that originally selected the repository has since been removed.
pub async fn registrations_from_common_dir(
    common_dir: &Path,
) -> io::Result<Vec<WorktreeRegistration>> {
    let out = run_git_dir_output(
        common_dir,
        &[
            OsStr::new("worktree"),
            OsStr::new("list"),
            OsStr::new("--porcelain"),
            OsStr::new("-z"),
        ],
    )
    .await?;
    if !out.status.success() {
        return Err(git_failure(&out));
    }
    parse_registrations(&out.stdout)
}

/// Registered worktree paths, kept for callers that do not need branch identity.
pub async fn list(repo: &Path) -> io::Result<Vec<PathBuf>> {
    Ok(registrations(repo)
        .await?
        .into_iter()
        .map(|registration| registration.path)
        .collect())
}

/// Is `path` inside a git work tree? Frontends gate the "use worktree" picker on baseline
/// availability (which carries the actual Git error); this remains for callers that only need the
/// boolean.
pub async fn is_git_repo(path: &Path) -> bool {
    run_git(
        path,
        &[OsStr::new("rev-parse"), OsStr::new("--is-inside-work-tree")],
    )
    .await
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn git(dir: &Path, args: &[&str]) {
        let osargs: Vec<&OsStr> = args.iter().map(|a| OsStr::new(*a)).collect();
        run_git(dir, &osargs).await.unwrap();
    }

    async fn git_stdout(dir: &Path, args: &[&str]) -> String {
        let osargs: Vec<&OsStr> = args.iter().map(|arg| OsStr::new(*arg)).collect();
        run_git(dir, &osargs).await.unwrap().trim().to_string()
    }

    async fn test_repo() -> Option<(PathBuf, PathBuf)> {
        if crate::provider::which("git").is_none() {
            eprintln!("git not found; skipping worktree test");
            return None;
        }

        let base = std::env::temp_dir().join(format!("codetwo-wt-{}", uuid::Uuid::new_v4()));
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]).await;
        git(&repo, &["config", "user.email", "t@t.dev"]).await;
        git(&repo, &["config", "user.name", "t"]).await;
        git(&repo, &["commit", "--allow-empty", "-qm", "init"]).await;
        Some((base, repo))
    }

    #[test]
    fn baseline_types_have_stable_wire_shape() {
        assert_eq!(
            serde_json::to_value(WorktreeBaseline::OriginDefault).unwrap(),
            serde_json::json!("origin_default")
        );
        let resolved = ResolvedWorktreeBaseline {
            kind: WorktreeBaseline::Current,
            reference: "trunk".to_string(),
            sha: "a".repeat(40),
            display: "trunk @ aaaaaaaa".to_string(),
        };
        let value = serde_json::to_value(resolved).unwrap();
        assert_eq!(value["ref"], "trunk");
        assert!(value.get("reference").is_none());
    }

    #[test]
    fn settings_round_trip_and_validate_the_root() {
        let data = std::env::temp_dir().join(format!(
            "codetwo-worktree-settings-{}",
            uuid::Uuid::new_v4()
        ));
        assert_eq!(load_settings(&data).unwrap(), WorktreeSettings::default());

        let root = data.join("managed");
        let saved = save_settings(
            &data,
            WorktreeSettings {
                root: Some(format!("  {}  ", root.display())),
                fetch_upstream: true,
                auto_delete: true,
                auto_delete_limit: 0,
            },
        )
        .unwrap();
        assert_eq!(saved.root.as_deref(), root.to_str());
        assert_eq!(saved.auto_delete_limit, 1);
        assert_eq!(load_settings(&data).unwrap(), saved);

        let error = save_settings(
            &data,
            WorktreeSettings {
                root: Some("relative/worktrees".into()),
                ..WorktreeSettings::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn configured_root_uses_a_repository_specific_container() {
        let configured = Path::new("/tmp/codetwo-worktrees");
        let first = session_container_dir_with_root(Path::new("/one/repo"), Some(configured));
        let second = session_container_dir_with_root(Path::new("/two/repo"), Some(configured));
        assert_ne!(first, second);
        assert_eq!(first.unwrap().parent(), Some(configured));
        assert_eq!(second.unwrap().parent(), Some(configured));
    }

    #[test]
    fn managed_session_checkout_accepts_current_and_legacy_layouts() {
        let repo = Path::new("source/repo");
        let container = Path::new("managed/repo-container");
        let session_id = "01234567-89ab-4cde-8f01-23456789abcd";

        assert!(is_managed_session_checkout(
            repo,
            container,
            &container.join(session_id).join("repo")
        ));
        assert!(is_managed_session_checkout(
            repo,
            container,
            &container.join("repo-legacy-session")
        ));
        assert!(is_managed_session_checkout(
            repo,
            container,
            &container.join(".codetwo-rollback-leftover")
        ));
        assert!(!is_managed_session_checkout(
            repo,
            container,
            &container.join(session_id).join("another-repo")
        ));
        assert!(!is_managed_session_checkout(
            repo,
            container,
            &container.join("not-a-uuid").join("repo")
        ));
        assert!(!is_managed_session_checkout(
            repo,
            container,
            &Path::new("outside").join(session_id).join("repo")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_repository_name_uses_the_repo_fallback() {
        use std::os::unix::ffi::OsStrExt;

        let root = PathBuf::from(OsStr::from_bytes(b"repo-\xff"));
        assert_eq!(session_checkout_name(&root), OsStr::new("repo"));
    }

    #[test]
    fn explicit_add_command_contains_sha_and_no_fetch() {
        let path = Path::new("/tmp/codetwo-explicit-worktree");
        let sha = "a".repeat(40);
        let args = add_from_sha_args(path, &sha);
        let actual: Vec<&str> = args.iter().map(|arg| arg.to_str().unwrap()).collect();
        assert_eq!(
            actual,
            vec![
                "worktree",
                "add",
                "--detach",
                "/tmp/codetwo-explicit-worktree",
                sha.as_str(),
            ]
        );
        assert!(!actual.contains(&"fetch"));
    }

    #[test]
    fn every_git_command_disables_promisor_lazy_fetch() {
        let command = git_command(Path::new("/tmp/codetwo-local-only"));
        let lazy_fetch = command
            .as_std()
            .get_envs()
            .find(|(name, _)| *name == OsStr::new("GIT_NO_LAZY_FETCH"))
            .and_then(|(_, value)| value);
        assert_eq!(lazy_fetch, Some(OsStr::new("1")));
        let command = git_dir_command(Path::new("/tmp/codetwo-local-only/.git"));
        let lazy_fetch = command
            .as_std()
            .get_envs()
            .find(|(name, _)| *name == OsStr::new("GIT_NO_LAZY_FETCH"))
            .and_then(|(_, value)| value);
        assert_eq!(lazy_fetch, Some(OsStr::new("1")));
    }

    #[test]
    fn porcelain_registration_parser_keeps_branch_identity() {
        let parsed = parse_registrations(
            b"worktree /repo\0HEAD aaaa\0branch refs/heads/main\0\0worktree /wt\0HEAD bbbb\0detached\0\0",
        )
        .unwrap();
        assert_eq!(
            parsed,
            vec![
                WorktreeRegistration {
                    path: PathBuf::from("/repo"),
                    branch: Some("refs/heads/main".into()),
                },
                WorktreeRegistration {
                    path: PathBuf::from("/wt"),
                    branch: None,
                },
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_identity_rejects_recycled_inode_birth_time_and_reads_legacy_receipts() {
        let directory = tempfile::tempdir().unwrap();
        let identity = DirectoryIdentity::capture(directory.path()).unwrap();
        let DirectoryIdentity::Unix {
            device,
            inode,
            birth_time_ns,
        } = identity
        else {
            unreachable!();
        };
        // Deterministically model inode reuse: same device/inode, different creation time.
        let recycled = DirectoryIdentity::Unix {
            device,
            inode,
            birth_time_ns: Some(birth_time_ns.unwrap_or(0).saturating_add(1)),
        };
        assert!(!recycled.matches_path(directory.path()).unwrap());
        let legacy: DirectoryIdentity = serde_json::from_value(serde_json::json!({
            "kind": "unix", "device": device, "inode": inode
        }))
        .unwrap();
        assert!(legacy.matches_path(directory.path()).unwrap());

        // Normal content changes must not invalidate a live session's directory receipt.
        std::fs::write(directory.path().join("file"), "changed").unwrap();
        assert!(identity.matches_path(directory.path()).unwrap());
        let restored: DirectoryIdentity =
            serde_json::from_value(serde_json::to_value(&identity).unwrap()).unwrap();
        assert_eq!(restored, identity);
    }

    #[test]
    fn directory_identity_has_stable_wire_shape() {
        let unix = DirectoryIdentity::Unix {
            device: 7,
            inode: 11,
            birth_time_ns: None,
        };
        assert_eq!(
            serde_json::to_value(unix).unwrap(),
            serde_json::json!({ "kind": "unix", "device": 7, "inode": 11 })
        );
        let windows = DirectoryIdentity::Windows {
            volume_serial_number: 13,
            file_index: 17,
        };
        assert_eq!(
            serde_json::to_value(windows).unwrap(),
            serde_json::json!({
                "kind": "windows",
                "volume_serial_number": 13,
                "file_index": 17,
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_never_moves_a_replacement_from_the_check_rename_window() {
        let base =
            std::env::temp_dir().join(format!("codetwo-quarantine-race-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("claimed");
        let displaced = base.join("displaced-original");
        let sentinel = path.join("replacement-sentinel");
        let claim = DirectoryClaim::create(&path).unwrap();

        let error = claim
            .quarantine_after_identity_check_for_test(|| {
                std::fs::rename(&path, &displaced).unwrap();
                std::fs::create_dir(&path).unwrap();
                std::fs::write(&sentinel, "must remain at the claimed name").unwrap();
            })
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error
            .to_string()
            .contains("no atomic compare-identity-and-rename"));
        assert_eq!(
            std::fs::read_to_string(&sentinel).unwrap(),
            "must remain at the claimed name"
        );
        assert!(displaced.is_dir());
        assert!(!std::fs::read_dir(&base).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".codetwo-rollback-")
        }));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn porcelain_registration_parser_preserves_non_utf8_paths() {
        use std::os::unix::ffi::OsStrExt;

        let parsed = parse_registrations(b"worktree /repo/\xff\0HEAD aaaa\0detached\0\0").unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path.as_os_str().as_bytes(), b"/repo/\xff");
    }

    #[tokio::test]
    async fn current_baseline_uses_head_commit_when_source_is_dirty() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let tracked = repo.join("tracked.txt");
        std::fs::write(&tracked, "committed\n").unwrap();
        git(&repo, &["add", "--", "tracked.txt"]).await;
        git(&repo, &["commit", "-qm", "tracked"]).await;
        let head = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        let branch = git_stdout(&repo, &["symbolic-ref", "--short", "HEAD"]).await;
        std::fs::write(&tracked, "dirty\n").unwrap();

        let resolved = resolve_baseline(&repo, WorktreeBaseline::Current)
            .await
            .unwrap();
        assert_eq!(resolved.kind, WorktreeBaseline::Current);
        assert_eq!(resolved.reference, branch);
        assert_eq!(resolved.sha, head);
        assert!(resolved.display.starts_with(&format!("{branch} @ ")));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn current_baseline_supports_detached_head() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let head = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        git(&repo, &["checkout", "--detach", "-q", &head]).await;

        let resolved = resolve_baseline(&repo, WorktreeBaseline::Current)
            .await
            .unwrap();
        assert_eq!(resolved.reference, "HEAD");
        assert_eq!(resolved.sha, head);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn origin_default_resolves_local_symbolic_target() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let head = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        git(&repo, &["update-ref", "refs/remotes/origin/trunk", &head]).await;
        git(
            &repo,
            &["symbolic-ref", ORIGIN_HEAD, "refs/remotes/origin/trunk"],
        )
        .await;

        let resolved = resolve_baseline(&repo, WorktreeBaseline::OriginDefault)
            .await
            .unwrap();
        assert_eq!(resolved.kind, WorktreeBaseline::OriginDefault);
        assert_eq!(resolved.reference, "refs/remotes/origin/trunk");
        assert_eq!(resolved.sha, head);
        assert!(resolved.display.starts_with("origin/trunk @ "));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn origin_default_rejects_missing_symbolic_ref_without_fallback() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let current = git_stdout(&repo, &["rev-parse", "HEAD"]).await;

        let error = resolve_baseline(&repo, WorktreeBaseline::OriginDefault)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains(ORIGIN_HEAD));
        assert!(!error.to_string().contains(&current));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn origin_default_rejects_dangling_symbolic_target() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        git(
            &repo,
            &["symbolic-ref", ORIGIN_HEAD, "refs/remotes/origin/missing"],
        )
        .await;

        let error = resolve_baseline(&repo, WorktreeBaseline::OriginDefault)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("refs/remotes/origin/missing"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn origin_default_uses_stale_local_ref_without_network_or_head_fallback() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let stale = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        git(&repo, &["update-ref", "refs/remotes/origin/trunk", &stale]).await;
        git(
            &repo,
            &["symbolic-ref", ORIGIN_HEAD, "refs/remotes/origin/trunk"],
        )
        .await;
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "/definitely/missing/codetwo-origin",
            ],
        )
        .await;
        git(&repo, &["commit", "--allow-empty", "-qm", "local advance"]).await;
        let current = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        assert_ne!(stale, current);

        let resolved = resolve_baseline(&repo, WorktreeBaseline::OriginDefault)
            .await
            .unwrap();
        assert_eq!(resolved.sha, stale);
        assert_ne!(resolved.sha, current);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn add_from_sha_branches_from_the_explicit_commit() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let baseline = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        git(&repo, &["commit", "--allow-empty", "-qm", "advance"]).await;
        let current = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        assert_ne!(baseline, current);

        let wt = base.join("wt-explicit");
        let created = add_from_sha(&repo, &wt, "feature/explicit", &baseline)
            .await
            .unwrap();
        assert_eq!(created.branch, "feature/explicit");
        assert_eq!(git_stdout(&wt, &["rev-parse", "HEAD"]).await, baseline);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn failed_creation_rollback_retains_its_path_and_branch() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let expected = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        let path = base.join("partial-worktree");
        let branch = "feature/rollback";
        let reference = branch_ref(branch);
        let created = add_from_sha(&repo, &path, branch, &expected).await.unwrap();

        let error = rollback_created(&repo, &created, &expected)
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("safe worktree quarantine is unavailable"));
        assert!(path.exists());
        assert_eq!(
            branch_target(&repo, &reference).await.unwrap().as_deref(),
            Some(expected.as_str())
        );
        git(&repo, &["update-ref", "-d", &reference, &expected]).await;
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn rollback_never_deletes_a_branch_that_moved() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let expected = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        let branch = "feature/moved-during-rollback";
        let reference = branch_ref(branch);
        let path = base.join("moved-worktree");
        let created = add_from_sha(&repo, &path, branch, &expected).await.unwrap();
        git(&repo, &["commit", "--allow-empty", "-qm", "advance"]).await;
        let moved = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        git(&repo, &["update-ref", &reference, &moved]).await;

        let error = rollback_created(&repo, &created, &expected)
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("safe worktree quarantine is unavailable"));
        assert!(path.exists());
        assert_eq!(
            branch_target(&repo, &reference).await.unwrap().as_deref(),
            Some(moved.as_str())
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn rollback_never_deletes_a_branch_used_by_another_worktree() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let expected = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        let branch = "feature/used-elsewhere";
        let reference = branch_ref(branch);
        git(&repo, &["update-ref", &reference, &expected]).await;
        let other = base.join("other-worktree");
        git(&repo, &["worktree", "add", other.to_str().unwrap(), branch]).await;

        let failed_path = base.join("failed-creator-path");
        let failed = Worktree {
            path: failed_path.clone(),
            branch: branch.to_string(),
            claim: DirectoryClaim::create(&failed_path).unwrap(),
        };
        let error = rollback_created(&repo, &failed, &expected)
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("safe worktree quarantine is unavailable"));
        assert!(failed_path.exists());
        assert_eq!(
            branch_target(&repo, &reference).await.unwrap().as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(
            git_stdout(&other, &["symbolic-ref", "HEAD"]).await,
            reference
        );
        remove(&repo, &other).await.unwrap();
        git(&repo, &["update-ref", "-d", &reference, &expected]).await;
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dangling_destination_symlink_is_never_claimed_or_removed() {
        use std::os::unix::fs::symlink;

        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let expected = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        let path = base.join("dangling-worktree");
        symlink(base.join("missing-target"), &path).unwrap();

        let error = add_from_sha(&repo, &path, "feature/dangling", &expected)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            branch_target(&repo, "refs/heads/feature/dangling")
                .await
                .unwrap(),
            None
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn add_for_session_from_origin_baseline_checks_out_its_stale_sha() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let origin_sha = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        git(
            &repo,
            &["update-ref", "refs/remotes/origin/trunk", &origin_sha],
        )
        .await;
        git(
            &repo,
            &["symbolic-ref", ORIGIN_HEAD, "refs/remotes/origin/trunk"],
        )
        .await;
        git(&repo, &["commit", "--allow-empty", "-qm", "local advance"]).await;
        let current = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        assert_ne!(origin_sha, current);
        let baseline = resolve_baseline(&repo, WorktreeBaseline::OriginDefault)
            .await
            .unwrap();

        let created = add_for_session_from_baseline(&repo, "origin-baseline", &baseline)
            .await
            .unwrap();
        assert_eq!(created.branch, "codetwo/origin-baseline");
        assert_eq!(
            git_stdout(&created.path, &["rev-parse", "HEAD"]).await,
            origin_sha
        );

        remove(&repo, &created.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn add_for_session_keeps_current_baseline_compatibility() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let current = git_stdout(&repo, &["rev-parse", "HEAD"]).await;

        let created = add_for_session(&repo, "legacy-current").await.unwrap();
        assert_eq!(created.branch, "codetwo/legacy-current");
        assert_eq!(
            created.path.file_name(),
            Some(OsStr::new("repo-legacy-current"))
        );
        assert_eq!(
            git_stdout(&created.path, &["rev-parse", "HEAD"]).await,
            current
        );

        remove(&repo, &created.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn uuid_session_checkout_keeps_the_repository_name_at_the_leaf() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let baseline = resolve_baseline(&repo, WorktreeBaseline::Current)
            .await
            .unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();

        let created = add_for_session_from_baseline(&repo, &session_id, &baseline)
            .await
            .unwrap();
        let root = repo.canonicalize().unwrap();
        let container = session_container_dir(&root).unwrap();

        assert_eq!(created.path.file_name(), Some(session_checkout_name(&root)));
        assert_eq!(
            created.path.parent().and_then(Path::file_name),
            Some(OsStr::new(&session_id))
        );
        assert_eq!(
            created.path.parent().and_then(Path::parent),
            Some(container.as_path())
        );
        assert!(is_managed_session_checkout(
            &root,
            &container,
            &created.path
        ));

        remove(&repo, &created.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn configured_root_keeps_the_repository_name_at_the_leaf() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let baseline = resolve_baseline(&repo, WorktreeBaseline::Current)
            .await
            .unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        let configured_root = base.join("managed");

        let created =
            add_for_session_from_baseline_in(&repo, &session_id, &baseline, Some(&configured_root))
                .await
                .unwrap();
        let root = repo.canonicalize().unwrap();
        let container = session_container_dir_with_root(&root, Some(&configured_root))
            .unwrap()
            .canonicalize()
            .unwrap();
        let session_parent = created.path.parent().unwrap().to_path_buf();

        assert_eq!(created.path.file_name(), Some(session_checkout_name(&root)));
        assert_eq!(session_parent.file_name(), Some(OsStr::new(&session_id)));
        assert_eq!(session_parent.parent(), Some(container.as_path()));
        assert!(is_managed_session_checkout(
            &root,
            &container,
            &created.path
        ));

        let common = common_dir(&repo).await.unwrap();
        discard_session_worktree(
            &common,
            &created.path,
            Some(created.directory_identity()),
            &created.branch,
        )
        .await
        .unwrap();
        assert!(!session_parent.exists());
        assert!(
            container.exists(),
            "the repository container remains reusable"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn uuid_session_parent_collision_is_never_reused() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let baseline = resolve_baseline(&repo, WorktreeBaseline::Current)
            .await
            .unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        let root = repo.canonicalize().unwrap();
        let container = session_container_dir(&root).unwrap();
        let session_parent = container.join(&session_id);
        std::fs::create_dir_all(&session_parent).unwrap();
        let sentinel = session_parent.join("sentinel");
        std::fs::write(&sentinel, "keep").unwrap();

        let error = add_for_session_from_baseline(&repo, &session_id, &baseline)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(sentinel.is_file());
        assert_eq!(
            branch_target(&repo, &branch_ref(&format!("codetwo/{session_id}")))
                .await
                .unwrap(),
            None
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn discard_removes_checkout_registration_and_branch() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let baseline = resolve_baseline(&repo, WorktreeBaseline::Current)
            .await
            .unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        let created = add_for_session_from_baseline(&repo, &session_id, &baseline)
            .await
            .unwrap();
        std::fs::write(created.path.join("dirty.txt"), "uncommitted\n").unwrap();
        let common = common_dir(&repo).await.unwrap();
        let session_parent = created.path.parent().unwrap().to_path_buf();
        let container = session_parent.parent().unwrap().to_path_buf();

        let outcome = discard_session_worktree(
            &common,
            &created.path,
            Some(created.directory_identity()),
            &created.branch,
        )
        .await
        .unwrap();

        assert!(outcome.removed_checkout);
        assert_eq!(
            outcome.deleted_branch.as_deref(),
            Some(created.branch.as_str())
        );
        assert!(!created.path.exists());
        assert!(
            !session_parent.exists(),
            "empty session parent should be removed"
        );
        assert!(!container.exists(), "empty container should be removed");
        assert_eq!(
            branch_target(&repo, &format!("refs/heads/codetwo/{session_id}"))
                .await
                .unwrap(),
            None
        );
        assert_eq!(registrations(&repo).await.unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn discard_refuses_branches_outside_the_session_namespace() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let common = common_dir(&repo).await.unwrap();
        for branch in ["main", "codetwo/", "codetwo/../main", "feature/x"] {
            let error = discard_session_worktree(&common, &repo, None, branch)
                .await
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{branch}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn discard_refuses_a_replacement_directory() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let baseline = resolve_baseline(&repo, WorktreeBaseline::Current)
            .await
            .unwrap();
        let created = add_for_session_from_baseline(&repo, "swapped", &baseline)
            .await
            .unwrap();
        let identity = created.directory_identity().clone();
        let common = common_dir(&repo).await.unwrap();
        let displaced = base.join("displaced");
        std::fs::rename(&created.path, &displaced).unwrap();
        std::fs::create_dir(&created.path).unwrap();

        let error =
            discard_session_worktree(&common, &created.path, Some(&identity), &created.branch)
                .await
                .unwrap_err();
        assert!(error.to_string().contains("directory identity changed"));
        assert!(created.path.exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn discard_refuses_a_checkout_registered_to_another_branch() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let head = git_stdout(&repo, &["rev-parse", "HEAD"]).await;
        let other = base.join("other-branch-wt");
        let created = add_from_sha(&repo, &other, "codetwo/other-session", &head)
            .await
            .unwrap();
        let common = common_dir(&repo).await.unwrap();

        let error = discard_session_worktree(
            &common,
            &other,
            Some(created.directory_identity()),
            "codetwo/not-this-one",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("expected branch"));
        assert!(other.exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn discard_of_an_externally_deleted_checkout_prunes_and_deletes_the_branch() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };
        let baseline = resolve_baseline(&repo, WorktreeBaseline::Current)
            .await
            .unwrap();
        let created = add_for_session_from_baseline(&repo, "gone-already", &baseline)
            .await
            .unwrap();
        let common = common_dir(&repo).await.unwrap();
        std::fs::remove_dir_all(&created.path).unwrap();

        let outcome = discard_session_worktree(&common, &created.path, None, &created.branch)
            .await
            .unwrap();

        assert!(!outcome.removed_checkout);
        assert_eq!(
            outcome.deleted_branch.as_deref(),
            Some("codetwo/gone-already")
        );
        assert_eq!(
            branch_target(&repo, "refs/heads/codetwo/gone-already")
                .await
                .unwrap(),
            None
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn add_list_remove_worktree() {
        let Some((base, repo)) = test_repo().await else {
            return;
        };

        let wt = base.join("wt-feature");
        let created = add(&repo, &wt, "feature/x").await.unwrap();
        assert!(created.path.is_dir());
        assert_eq!(created.branch, "feature/x");

        let listed = list(&repo).await.unwrap();
        assert!(listed.iter().any(|p| p.ends_with("wt-feature")));

        remove(&repo, &wt).await.unwrap();
        assert!(!wt.exists());

        let _ = std::fs::remove_dir_all(&base);
    }
}
