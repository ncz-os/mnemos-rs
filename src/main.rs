//! mnemosctl — MNEMOS desktop CLI
//!
//! Replaces three Python desktop helpers in `~/mnemos-desktop/`:
//!   - `sync_from_pythia.py` -> `mnemosctl sync pull --peer pythia`
//!   - `sync_to_pythia.py`   -> `mnemosctl sync push --peer pythia`
//!   - `create_local.py`     -> `mnemosctl create --category facts ...`
//!
//! Plus net-new helpers: status / search / peer / mcp-stdio.
//!
//! See `project_mnemos_rust_clients.md` in MNEMOS auto-memory for the
//! full charter and Python/Rust scope decision.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser, Debug)]
#[command(name = "mnemosctl", version, about, long_about = None)]
struct Cli {
    /// Base URL of the MNEMOS server (default: http://localhost:5002)
    #[arg(
        long,
        env = "MNEMOS_BASE",
        global = true,
        default_value = "http://localhost:5002"
    )]
    base: String,

    /// Bearer token for auth (default: read from MNEMOS_API_KEY env)
    #[arg(long, env = "MNEMOS_API_KEY", global = true, hide_env_values = true)]
    token: Option<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Show /health + /stats summary
    Status {
        /// Print raw JSON from /health and /stats
        #[arg(long)]
        raw: bool,
    },

    /// Search memories
    Search {
        /// Query string
        query: String,

        /// Max results
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: u32,
    },

    /// Create a memory on the connected mnemos
    Create {
        /// Memory content
        #[arg(long)]
        content: String,

        /// Category (facts, solutions, architecture, ...)
        #[arg(long, default_value = "facts")]
        category: String,
    },

    /// Sync memories with a peer
    Sync {
        #[command(subcommand)]
        direction: SyncDirection,
    },

    /// Manage peer connections
    Peer {
        #[command(subcommand)]
        action: PeerAction,
    },

    /// Run the MCP stdio bridge (spawned by Claude Code per session)
    McpStdio,
}

#[derive(Subcommand, Debug)]
enum SyncDirection {
    /// Pull memories from peer into local
    Pull {
        /// Peer name (configured via `mnemosctl peer add`)
        #[arg(long)]
        peer: String,
    },
    /// Push local memories to peer
    Push {
        #[arg(long)]
        peer: String,
    },
}

#[derive(Subcommand, Debug)]
enum PeerAction {
    /// Add a peer
    Add {
        /// Peer URL
        url: String,

        /// Peer name (defaults to host portion of URL)
        #[arg(long)]
        name: Option<String>,

        /// Bearer token for the peer
        #[arg(long, hide_env_values = true)]
        token: Option<String>,
    },
    /// List configured peers
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Status { raw } => commands::status(&cli.base, cli.token.as_deref(), raw).await,
        Cmd::Search { query, limit } => {
            commands::search(&cli.base, cli.token.as_deref(), &query, limit).await
        }
        Cmd::Create { content, category } => {
            commands::create(&cli.base, cli.token.as_deref(), &content, &category).await
        }
        Cmd::Sync { direction } => commands::sync(&cli.base, cli.token.as_deref(), direction).await,
        Cmd::Peer { action } => commands::peer(action).await,
        Cmd::McpStdio => commands::mcp_stdio(&cli.base, cli.token.as_deref()).await,
    }
}
