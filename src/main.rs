//! mnemos-desktop — MNEMOS desktop CLI
//!
//! Provides status, search, memory creation, and peer configuration helpers.
//!
//! See `project_mnemos_rust_clients.md` in MNEMOS auto-memory for the
//! full charter and Python/Rust scope decision.

use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

mod commands;
mod secret;
mod transport;

#[derive(Parser, Debug)]
#[command(name = "mnemos-desktop", version, about, long_about = None)]
struct Cli {
    /// Base URL of the MNEMOS server (default: http://localhost:5002)
    #[arg(
        long,
        env = "MNEMOS_BASE",
        global = true,
        default_value = "http://localhost:5002"
    )]
    base: String,

    /// Deprecated and unsafe: credential values are rejected on the command line.
    #[arg(long = "token", global = true, hide = true)]
    token_arg: Option<String>,

    /// TOML credentials file containing `token` and/or `peer_token` (mode 0600).
    #[arg(
        long,
        env = "MNEMOS_CREDENTIALS_FILE",
        global = true,
        value_name = "PATH"
    )]
    credentials_file: Option<PathBuf>,

    /// Explicitly allow bearer credentials over plaintext HTTP to non-loopback hosts.
    #[arg(long, global = true)]
    allow_insecure_http: bool,

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

    /// Manage peer connections
    Peer {
        #[command(subcommand)]
        action: PeerAction,
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

        /// Deprecated and unsafe: credential values are rejected on the command line.
        #[arg(long = "peer-token", hide = true)]
        peer_token_arg: Option<String>,

        /// Drop the previously-saved token. Required when re-adding a
        /// peer to make it anonymous; without this flag, omitting a token
        /// source preserves the existing token.
        #[arg(long = "clear-token", conflicts_with = "peer_token_arg")]
        clear_token: bool,

        /// Carry the existing bearer across a URL change. Required
        /// when changing a peer's URL without supplying a new token,
        /// because silently re-binding a bearer to a different host
        /// would leak credentials across trust boundaries. Same-URL
        /// retries do NOT need this flag.
        #[arg(
            long = "preserve-token",
            conflicts_with = "peer_token_arg",
            conflicts_with = "clear_token"
        )]
        preserve_token: bool,
    },
    /// List configured peers
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    reject_credential_arguments();
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).map_err(|e| {
        e.exit();
    });
    let cli = cli.unwrap_or_else(|_| unreachable!("from_arg_matches exits on error"));

    let credentials = secret::Credentials::load(cli.credentials_file.as_deref())?;

    match cli.cmd {
        Cmd::Status { raw } => {
            transport::require_secure_for_credentials(
                &cli.base,
                credentials.token.is_some(),
                cli.allow_insecure_http,
            )?;
            commands::status(&cli.base, credentials.token.as_deref(), raw).await
        }
        Cmd::Search { query, limit, raw } => {
            transport::require_secure_for_credentials(
                &cli.base,
                credentials.token.is_some(),
                cli.allow_insecure_http,
            )?;
            commands::search(&cli.base, credentials.token.as_deref(), &query, limit, raw).await
        }
        Cmd::Create {
            content,
            category,
            subcategory,
            permission_mode,
            raw,
        } => {
            transport::require_secure_for_credentials(
                &cli.base,
                credentials.token.is_some(),
                cli.allow_insecure_http,
            )?;
            commands::create(
                &cli.base,
                credentials.token.as_deref(),
                content.as_deref(),
                &category,
                subcategory.as_deref(),
                permission_mode,
                raw,
            )
            .await
        }
        Cmd::Peer { action } => {
            match &action {
                PeerAction::Add { url, .. } => {
                    // Peer configuration can preserve a server-stored bearer even when no
                    // credential is supplied locally. Protect both the management request
                    // and the peer destination conservatively.
                    transport::require_secure_for_credentials(
                        &cli.base,
                        true,
                        cli.allow_insecure_http,
                    )?;
                    transport::require_secure_for_credentials(url, true, cli.allow_insecure_http)?;
                }
                PeerAction::List => transport::require_secure_for_credentials(
                    &cli.base,
                    credentials.token.is_some(),
                    cli.allow_insecure_http,
                )?,
            }
            let suppress_peer_token = matches!(
                &action,
                PeerAction::List
                    | PeerAction::Add {
                        clear_token: true,
                        ..
                    }
                    | PeerAction::Add {
                        preserve_token: true,
                        ..
                    }
            );
            let peer_token = (!suppress_peer_token)
                .then_some(credentials.peer_token)
                .flatten();
            commands::peer(&cli.base, credentials.token.as_deref(), peer_token, action).await
        }
    }
}

fn reject_credential_arguments() {
    for argument in std::env::args_os().skip(1) {
        let argument = argument.to_string_lossy();
        if argument == "--" {
            break;
        }
        let message = if argument == "--token" || argument.starts_with("--token=") {
            Some(
                "bearer credentials are not accepted in argv; set MNEMOS_TOKEN (or \
                 MNEMOS_API_KEY) or use --credentials-file PATH (mode 0600)",
            )
        } else if argument == "--peer-token" || argument.starts_with("--peer-token=") {
            Some(
                "peer bearer credentials are not accepted in argv; set MNEMOS_PEER_TOKEN or \
                 use --credentials-file PATH (mode 0600)",
            )
        } else {
            None
        };
        if let Some(message) = message {
            eprintln!("error: {message}");
            std::process::exit(2);
        }
    }
}
