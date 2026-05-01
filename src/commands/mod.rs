use anyhow::Result;

#[cfg(not(unix))]
use crate::PeerAction;
use crate::SyncDirection;

mod create;
#[cfg(unix)]
mod peer;
mod search;
mod status;

pub use create::create;
pub use search::search;
pub use status::status;

#[cfg(unix)]
pub use peer::peer;

/// peer add/list relies on Unix file-permission and flock(2) primitives
/// (0600 mode, advisory locking, symlink_metadata refusal). The fleet
/// targets — macOS, Linux, WSL2 (Linux variant) — all satisfy this.
/// Native Windows is intentionally out of scope; the proper fix would
/// be MoveFileExW/ReplaceFileW + LockFileEx + ACL hardening, but no
/// fleet host runs Windows-native mnemosctl today (TYPHON's Windows
/// boot uses WSL2 Debian for any mnemos work).
#[cfg(not(unix))]
pub async fn peer(_action: PeerAction) -> Result<()> {
    anyhow::bail!(
        "peer add/list requires Unix file primitives (mode 0600, flock, symlink_metadata). \
         Native Windows is not a supported target — use the Linux build under WSL2 instead."
    )
}

pub async fn sync(_base: &str, _token: Option<&str>, _dir: SyncDirection) -> Result<()> {
    anyhow::bail!("sync: not implemented in slice 1 (scaffold only)")
}

pub async fn mcp_stdio(_base: &str, _token: Option<&str>) -> Result<()> {
    anyhow::bail!("mcp-stdio: not implemented in slice 1 (scaffold only)")
}
