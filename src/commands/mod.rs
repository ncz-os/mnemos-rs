use anyhow::Result;

use crate::{PeerAction, SyncDirection};

mod create;
mod search;
mod status;

pub use create::create;
pub use search::search;
pub use status::status;

pub async fn sync(_base: &str, _token: Option<&str>, _dir: SyncDirection) -> Result<()> {
    anyhow::bail!("sync: not implemented in slice 1 (scaffold only)")
}

pub async fn peer(_action: PeerAction) -> Result<()> {
    anyhow::bail!("peer: not implemented in slice 1 (scaffold only)")
}

pub async fn mcp_stdio(_base: &str, _token: Option<&str>) -> Result<()> {
    anyhow::bail!("mcp-stdio: not implemented in slice 1 (scaffold only)")
}
