use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::PeerAction;

/// Per-process counter ensures distinct tempfile names within a single
/// CLI run (theoretical: a future call site that saves twice in one
/// process). Combined with PID and nanos, the path collision risk is
/// negligible.
static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Persisted peer record. `token` is optional (some peers accept anonymous
/// reads, e.g. a local desktop instance with auth disabled).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Peer {
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PeersFile {
    #[serde(default, rename = "peer")]
    peers: Vec<Peer>,
}

pub async fn peer(action: PeerAction) -> Result<()> {
    let (path, mnemosctl_owns_dir) = peers_path()?;
    match action {
        PeerAction::Add {
            url,
            name,
            peer_token,
            clear_token,
            preserve_token,
        } => {
            add(
                &path,
                mnemosctl_owns_dir,
                &url,
                name,
                peer_token,
                clear_token,
                preserve_token,
            )
            .await
        }
        PeerAction::List => list(&path).await,
    }
}

async fn add(
    path: &Path,
    mnemosctl_owns_dir: bool,
    url: &str,
    name: Option<String>,
    peer_token: Option<String>,
    clear_token: bool,
    preserve_token: bool,
) -> Result<()> {
    if url.trim().is_empty() {
        eprintln!("error: peer URL cannot be empty");
        std::process::exit(2);
    }
    let resolved_name = match name {
        Some(n) if !n.trim().is_empty() => n,
        _ => default_name_from_url(url),
    };
    // Treat empty/whitespace-only --peer-token as absent so spurious
    // "" never gets persisted. (clap doesn't reject empty strings.)
    let new_token = peer_token.and_then(|t| {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(t)
        }
    });

    // Advisory lock spans the entire load-modify-save transaction so
    // concurrent `peer add` invocations cannot last-write-wins drop a
    // peer or resurrect a cleared token. The lock is held until _lock
    // drops at the end of this function. fs2 uses flock(2) on Unix and
    // LockFileEx on Windows; both are advisory but enforced because
    // mnemosctl is the only writer of peers.toml.
    let _lock = acquire_lock(path).await?;

    let mut file = load(path).await?;

    if let Some(existing) = file.peers.iter_mut().find(|p| p.name == resolved_name) {
        let was_url = existing.url.clone();
        let was_token = existing.token.clone();
        let url_changed = was_url != url;

        // Token merge policy:
        //   --peer-token X      -> set to X (any URL)
        //   --clear-token       -> drop existing token (any URL)
        //   --preserve-token    -> carry existing token across URL change
        //   none, same URL      -> preserve existing token (idempotent retry)
        //   none, NEW URL       -> REJECT: silently rebinding a bearer
        //                         to a new endpoint would leak the
        //                         credential across a trust boundary.
        if url_changed
            && new_token.is_none()
            && !clear_token
            && !preserve_token
            && was_token.is_some()
        {
            eprintln!(
                "error: changing the URL for peer '{resolved_name}' (was {was_url}, now {url}) \
                 requires explicit token handling. Pass one of:\n  \
                 --peer-token <NEW_TOKEN>   set a new token for the new endpoint\n  \
                 --clear-token              drop the saved token (anonymous peer)\n  \
                 --preserve-token           carry the existing token across (only if both \
                                            endpoints belong to the same trust boundary)"
            );
            std::process::exit(2);
        }

        let token = if let Some(t) = new_token.clone() {
            Some(t)
        } else if clear_token {
            None
        } else {
            // Preserve path: same-URL retry OR explicit --preserve-token.
            was_token.clone()
        };
        *existing = Peer {
            name: resolved_name.clone(),
            url: url.to_string(),
            token: token.clone(),
        };
        save(path, mnemosctl_owns_dir, &file).await?;
        let token_changed = was_token != token;
        match (url_changed, token_changed, clear_token) {
            (true, _, _) => println!("Updated peer '{resolved_name}' -> {url}"),
            (false, true, true) => println!("Cleared token for peer '{resolved_name}'"),
            (false, true, false) => println!("Updated peer '{resolved_name}' (token refreshed)"),
            (false, false, _) => println!("Peer '{resolved_name}' already at {url}"),
        }
    } else {
        let entry = Peer {
            name: resolved_name.clone(),
            url: url.to_string(),
            token: new_token,
        };
        file.peers.push(entry);
        save(path, mnemosctl_owns_dir, &file).await?;
        println!("Added peer '{resolved_name}' -> {url}");
    }
    Ok(())
}

async fn list(path: &Path) -> Result<()> {
    let file = load(path).await?;
    if file.peers.is_empty() {
        println!("No peers configured. Add one with: mnemosctl peer add <URL> --name <NAME>");
        return Ok(());
    }
    println!("{:<20} {:<6} URL", "NAME", "TOKEN");
    for p in &file.peers {
        let token_state = if p.token.is_some() { "set" } else { "—" };
        println!("{:<20} {:<6} {}", p.name, token_state, p.url);
    }
    Ok(())
}

fn default_name_from_url(url: &str) -> String {
    // Strip scheme then take the host portion before any port or path.
    let trimmed = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host: String = trimmed
        .chars()
        .take_while(|c| !matches!(*c, ':' | '/'))
        .collect();
    if host.is_empty() {
        "peer".to_string()
    } else {
        host
    }
}

/// Drop guard that releases the advisory lock when the binding goes
/// out of scope. fs2's lock travels with the std::fs::File handle, so
/// dropping the file releases the flock.
struct LockGuard {
    _file: std::fs::File,
}

async fn acquire_lock(peers_path: &Path) -> Result<LockGuard> {
    use fs2::FileExt;
    let parent = peers_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    create_dir_durably(&parent)
        .await
        .with_context(|| format!("create {}", parent.display()))?;
    let lock_path = parent.join("peers.toml.lock");

    // Spawn-blocking because fs2 uses sync syscalls. The lock file is
    // tiny and short-lived; the open + flock is microseconds.
    tokio::task::spawn_blocking(move || -> Result<LockGuard> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("open {}", lock_path.display()))?;
        file.try_lock_exclusive().or_else(|_| {
            // Contention path — block until the holder releases. Keeps
            // CLI semantics simple: a peer add waits for an in-flight
            // peer add to finish rather than failing fast.
            file.lock_exclusive()
                .with_context(|| format!("flock {}", lock_path.display()))
        })?;
        Ok(LockGuard { _file: file })
    })
    .await
    .context("lock acquisition task panicked")?
}

/// Where peers.toml lives. The bool flag distinguishes a directory we
/// own (we created or fully manage — safe to chmod 0700) from a
/// directory the user supplied via $MNEMOS_HOME (leave perms alone:
/// chmodding $HOME or a synced folder would be hostile).
///
/// Path validation: reject empty or relative MNEMOS_HOME, reject
/// empty HOME. An empty env var would otherwise resolve to a relative
/// `peers.toml` in the caller's cwd, persisting bearer tokens to the
/// nearest source repo or synced directory.
fn peers_path() -> Result<(PathBuf, bool)> {
    if let Ok(home) = std::env::var("MNEMOS_HOME") {
        let trimmed = home.trim();
        if trimmed.is_empty() {
            anyhow::bail!(
                "MNEMOS_HOME is empty; refusing to persist peer secrets to a relative path. \
                 Unset it to fall back to $HOME/.mnemos, or set it to an absolute directory."
            );
        }
        let path = PathBuf::from(trimmed);
        if !path.is_absolute() {
            anyhow::bail!(
                "MNEMOS_HOME={trimmed} is not absolute; peer secrets must live at a fixed path. \
                 Use an absolute directory (e.g. /var/lib/mnemos)."
            );
        }
        return Ok((path.join("peers.toml"), false));
    }
    let home = std::env::var("HOME")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .context("HOME is not set or empty; cannot locate ~/.mnemos/")?;
    let home_path = PathBuf::from(&home);
    if !home_path.is_absolute() {
        anyhow::bail!(
            "HOME={home} is not absolute; refusing to persist peer secrets to a relative path."
        );
    }
    Ok((home_path.join(".mnemos").join("peers.toml"), true))
}

async fn load(path: &Path) -> Result<PeersFile> {
    // Refuse to read symlinks — peers.toml is a secret file; following a
    // symlink could redirect a privileged owner-write to an unintended
    // path, or expose a token to a foreign caller. Owner regular file
    // only.
    #[cfg(unix)]
    {
        match fs::symlink_metadata(path).await {
            Ok(meta) => {
                let ft = meta.file_type();
                if ft.is_symlink() {
                    anyhow::bail!(
                        "{} is a symlink; refusing to read peers config through one",
                        path.display()
                    );
                }
                if meta.is_file() {
                    tighten_file_mode_if_needed(path).await?;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err).with_context(|| format!("stat {}", path.display())),
        }
    }
    match fs::read_to_string(path).await {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(PeersFile::default()),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

#[cfg(unix)]
async fn tighten_file_mode_if_needed(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(path)
        .await
        .with_context(|| format!("stat {}", path.display()))?;
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o600 {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)
            .await
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}

async fn save(path: &Path, mnemosctl_owns_dir: bool, file: &PeersFile) -> Result<()> {
    // peers.toml stores bearer tokens. Persistence rules:
    // 1. File mode 0600 + (when we own the dir) parent 0700.
    // 2. Atomic replace via per-writer tempfile + rename so a crash
    //    mid-write cannot truncate the existing config to empty AND
    //    two concurrent writers won't unlink each other's tempfile.
    // 3. fsync the file (write_secret_file) AND fsync the parent
    //    directory after rename — without the parent fsync, the new
    //    directory entry is not durable and a crash can roll back to
    //    the old peers.toml, including resurrecting a token the user
    //    was just told was cleared.
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    create_dir_durably(parent)
        .await
        .with_context(|| format!("create {}", parent.display()))?;
    if mnemosctl_owns_dir {
        tighten_dir_mode(parent).await?;
    }
    let text = toml::to_string_pretty(file).context("serialize peers.toml")?;

    let tmp = unique_tempfile(path);
    write_secret_file(&tmp, &text)
        .await
        .with_context(|| format!("write {}", tmp.display()))?;
    let rename_result = fs::rename(&tmp, path)
        .await
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()));
    if let Err(err) = rename_result {
        let _ = fs::remove_file(&tmp).await;
        return Err(err);
    }
    // Durability: parent-dir fsync after rename. tokio::fs::File can
    // open a directory on Unix and sync_all() it — that flushes the
    // dirent so the rename is on-disk before we report success.
    let dir_handle = fs::File::open(parent)
        .await
        .with_context(|| format!("open {} for fsync", parent.display()))?;
    dir_handle
        .sync_all()
        .await
        .with_context(|| format!("fsync {}", parent.display()))?;
    Ok(())
}

fn unique_tempfile(path: &Path) -> PathBuf {
    let pid = process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = SAVE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let suffix = format!(".tmp.{pid}.{nanos}.{counter}");
    let mut buf = path.as_os_str().to_os_string();
    buf.push(suffix);
    PathBuf::from(buf)
}

#[cfg(unix)]
/// Crash-durable equivalent of `create_dir_all`. For each new
/// directory created, fsync its PARENT so the dirent is on-disk
/// before this returns. Without this, a crash after `peer add`
/// reports success can roll back the entire ~/.mnemos creation
/// (the dirent for `.mnemos` lives in $HOME and would never have
/// been flushed by the post-rename peers.toml fsync alone).
async fn create_dir_durably(target: &Path) -> Result<()> {
    let mut to_create: Vec<PathBuf> = Vec::new();
    let mut cur = target.to_path_buf();
    let existing_boundary: Option<PathBuf> = loop {
        match fs::try_exists(&cur).await {
            Ok(true) => break Some(cur.clone()),
            Ok(false) => {}
            Err(err) => {
                return Err(err).with_context(|| format!("stat {}", cur.display()));
            }
        }
        let parent = match cur.parent() {
            Some(p) => p.to_path_buf(),
            None => break None,
        };
        to_create.push(cur.clone());
        cur = parent;
    };

    // Concurrent-first-run race: another process may have created the
    // boundary directory and not yet fsynced its parent. We don't
    // know — flush it ourselves before relying on it. (No-op if the
    // boundary is /, which has no parent.)
    if let Some(boundary) = existing_boundary {
        if let Some(parent) = boundary.parent() {
            let handle = fs::File::open(parent)
                .await
                .with_context(|| format!("open {} for boundary fsync", parent.display()))?;
            handle
                .sync_all()
                .await
                .with_context(|| format!("fsync {}", parent.display()))?;
        }
    }
    // Walk top-down (deepest-existing first) so each create's parent
    // already exists before we open it for fsync.
    for dir in to_create.iter().rev() {
        match fs::create_dir(dir).await {
            Ok(_) => {}
            // Race: another process / earlier iteration created it.
            // Idempotent — keep going.
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => {
                return Err(err).with_context(|| format!("create {}", dir.display()));
            }
        }
        if let Some(parent) = dir.parent() {
            let handle = fs::File::open(parent)
                .await
                .with_context(|| format!("open {} for fsync", parent.display()))?;
            handle
                .sync_all()
                .await
                .with_context(|| format!("fsync {}", parent.display()))?;
        }
    }
    Ok(())
}

async fn tighten_dir_mode(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // symlink_metadata so a `~/.mnemos -> ~/Documents/synced` setup
    // does not silently chmod the symlink target to 0700. If the
    // canonical default path is itself a symlink, treat the dir as
    // user-supplied and leave perms alone.
    let lmeta = fs::symlink_metadata(dir)
        .await
        .with_context(|| format!("lstat {}", dir.display()))?;
    if lmeta.file_type().is_symlink() {
        return Ok(());
    }
    let mode = lmeta.permissions().mode() & 0o777;
    if mode != 0o700 {
        let mut perms = lmeta.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(dir, perms)
            .await
            .with_context(|| format!("chmod 700 {}", dir.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
async fn write_secret_file(path: &Path, contents: &str) -> Result<()> {
    // tokio::fs::OpenOptions exposes `.mode()` directly on Unix; no
    // OpenOptionsExt import needed.
    use tokio::io::AsyncWriteExt;

    // Pre-truncate any stale tempfile from an earlier crash, then create
    // with mode 0600 directly (umask cannot widen this past the OS allow).
    let _ = fs::remove_file(path).await;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .await
        .with_context(|| format!("open {} for write (mode 0600)", path.display()))?;
    file.write_all(contents.as_bytes())
        .await
        .with_context(|| format!("write {}", path.display()))?;
    file.flush()
        .await
        .with_context(|| format!("flush {}", path.display()))?;
    file.sync_all()
        .await
        .with_context(|| format!("fsync {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_strips_scheme_and_port() {
        assert_eq!(
            default_name_from_url("http://192.168.207.67:5002"),
            "192.168.207.67"
        );
        assert_eq!(
            default_name_from_url("https://pythia.local/v1"),
            "pythia.local"
        );
        assert_eq!(
            default_name_from_url("https://pythia.local"),
            "pythia.local"
        );
        assert_eq!(default_name_from_url(""), "peer");
    }
}
