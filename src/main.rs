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
use clap::parser::ValueSource;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

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

    /// Bearer token for auth (default: read from MNEMOS_API_KEY env).
    /// global = true so authenticated subcommands accept --token in
    /// either position (`mnemosctl --token X status` AND `mnemosctl
    /// status --token X`). Misuse with `peer add ... --token X` is
    /// caught at runtime via ArgMatches::value_source() — a CLI-sourced
    /// token paired with Cmd::Peer is rejected, while env-sourced
    /// tokens (MNEMOS_API_KEY) remain harmless because peer dispatch
    /// never forwards cli.token.
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

        /// Print raw JSON response
        #[arg(long)]
        raw: bool,
    },

    /// Create a memory on the connected mnemos
    Create {
        /// Memory content (omit to read from stdin)
        #[arg(long)]
        content: Option<String>,

        /// Category (facts, solutions, architecture, ...)
        #[arg(long, default_value = "facts")]
        category: String,

        /// Subcategory for scoped retrieval
        #[arg(long)]
        subcategory: Option<String>,

        /// Octal-style Unix permission digits (0-777). 644 = federation-visible.
        #[arg(long)]
        permission_mode: Option<u16>,

        /// Print raw JSON response from the server
        #[arg(long)]
        raw: bool,
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

        /// Bearer token for the peer. Distinct flag name from the
        /// global --token so the server bearer (sourced from
        /// MNEMOS_API_KEY) does not silently leak into peer records.
        #[arg(long = "peer-token", hide_env_values = true)]
        peer_token: Option<String>,

        /// Drop the previously-saved token. Required when re-adding a
        /// peer to make it anonymous; without this flag, omitting
        /// --peer-token preserves the existing token.
        #[arg(long = "clear-token", conflicts_with = "peer_token")]
        clear_token: bool,

        /// Carry the existing bearer across a URL change. Required
        /// when changing a peer's URL without supplying a new token,
        /// because silently re-binding a bearer to a different host
        /// would leak credentials across trust boundaries. Same-URL
        /// retries do NOT need this flag.
        #[arg(
            long = "preserve-token",
            conflicts_with = "peer_token",
            conflicts_with = "clear_token"
        )]
        preserve_token: bool,
    },
    /// List configured peers
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse via matches+from_arg_matches so we can introspect WHICH
    // source populated `--token`. Default-from-env is fine for most
    // commands; CLI-supplied --token paired with `peer add` is a
    // silent footgun (server bearer would be persisted as the peer's
    // token if forwarded blindly, or dropped if not). Reject explicitly.
    let matches = Cli::command().get_matches();
    let token_from_cli = matches.value_source("token") == Some(ValueSource::CommandLine);
    let cli = Cli::from_arg_matches(&matches).map_err(|e| {
        e.exit();
    });
    let cli = cli.unwrap_or_else(|_| unreachable!("from_arg_matches exits on error"));

    if token_from_cli && matches!(cli.cmd, Cmd::Peer { .. }) {
        eprintln!(
            "error: --token is for server auth, not peer credentials. \
             Use `peer add ... --peer-token TOK` to attach a token to a peer."
        );
        std::process::exit(2);
    }

    match cli.cmd {
        Cmd::Status { raw } => commands::status(&cli.base, cli.token.as_deref(), raw).await,
        Cmd::Search { query, limit, raw } => {
            commands::search(&cli.base, cli.token.as_deref(), &query, limit, raw).await
        }
        Cmd::Create {
            content,
            category,
            subcategory,
            permission_mode,
            raw,
        } => {
            commands::create(
                &cli.base,
                cli.token.as_deref(),
                content.as_deref(),
                &category,
                subcategory.as_deref(),
                permission_mode,
                raw,
            )
            .await
        }
        Cmd::Sync { direction } => commands::sync(&cli.base, cli.token.as_deref(), direction).await,
        Cmd::Peer { action } => commands::peer(action).await,
        Cmd::McpStdio => commands::mcp_stdio(&cli.base, cli.token.as_deref()).await,
    }
}
