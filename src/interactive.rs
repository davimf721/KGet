use crate::{Ctx, Job};
use kget::config::Config;
use kget::protocol::Protocol;
use kget::queue::DownloadHistory;
use kget::utils;
use kget::ytdlp::VideoQuality;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

// ─────────────────────────────────────────────────────────────────────────────
// ASCII Banner
// ─────────────────────────────────────────────────────────────────────────────

const BANNER: &str = r#"
  ██╗  ██╗ ██████╗ ███████╗████████╗
  ██║ ██╔╝██╔════╝ ██╔════╝╚══██╔══╝
  █████╔╝ ██║  ███╗█████╗     ██║
  ██╔═██╗ ██║   ██║██╔══╝     ██║
  ██║  ██╗╚██████╔╝███████╗   ██║
  ╚═╝  ╚═╝ ╚═════╝ ╚══════╝   ╚═╝
"#;

// ─────────────────────────────────────────────────────────────────────────────
// Parsed download arguments from the REPL line
// ─────────────────────────────────────────────────────────────────────────────

struct DownloadArgs {
    url: String,
    output: Option<String>,
    quiet: bool,
    advanced: bool,
    forced: Option<Protocol>,
    sha256: Option<String>,
    headers: Vec<String>,
    resume: bool,
    quality: String,
}

/// Parse flags and URL from the tokens after the `download` command.
///
/// Accepted flags:
/// ```text
/// -a / --advanced / --turbo     parallel multi-connection download
/// -o / --output <path>          destination file or directory
/// -q / --quiet                  suppress progress output
/// -c / --continue               resume a partial download
/// -H / --header "Name: value"   extra HTTP header (repeatable)
/// --sha256 <hash>               verify file hash after download
/// --quality <q>                 yt-dlp quality (best, 1080p, 720p, audio…)
/// --ftp / --sftp / --webdav     force a protocol
/// --torrent                     force torrent/magnet handling
/// --metalink                    treat source as a Metalink manifest
/// --ytdlp                       route through yt-dlp
/// ```
fn parse_download_args(parts: &[&str]) -> Result<DownloadArgs, String> {
    let mut args = DownloadArgs {
        url: String::new(),
        output: None,
        quiet: false,
        advanced: false,
        forced: None,
        sha256: None,
        headers: Vec::new(),
        resume: false,
        quality: "best".into(),
    };

    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            "-a" | "--advanced" | "--turbo" => args.advanced = true,
            "-q" | "--quiet" => args.quiet = true,
            "-c" | "--continue" => args.resume = true,
            "--ftp" => args.forced = Some(Protocol::Ftp),
            "--sftp" => args.forced = Some(Protocol::Sftp),
            "--webdav" => args.forced = Some(Protocol::WebDav),
            "--torrent" => args.forced = Some(Protocol::Torrent),
            "--metalink" => args.forced = Some(Protocol::Metalink),
            "--ytdlp" => args.forced = Some(Protocol::Video),
            "-o" | "-O" | "--output" => {
                i += 1;
                if i >= parts.len() {
                    return Err(format!("'{}' requires a path argument", parts[i - 1]));
                }
                args.output = Some(utils::expand_tilde(parts[i]));
            }
            "-H" | "--header" => {
                i += 1;
                if i >= parts.len() {
                    return Err("'--header' requires a \"Name: value\" argument".to_string());
                }
                args.headers.push(parts[i].to_string());
            }
            "--quality" => {
                i += 1;
                if i >= parts.len() {
                    return Err("'--quality' requires a value".to_string());
                }
                args.quality = parts[i].to_string();
            }
            "--sha256" => {
                i += 1;
                if i >= parts.len() {
                    return Err("'--sha256' requires a hash argument".to_string());
                }
                args.sha256 = Some(parts[i].to_string());
            }
            token if !token.starts_with('-') => {
                if !args.url.is_empty() {
                    return Err(format!(
                        "Unexpected extra argument '{}'. Only one URL is supported.",
                        token
                    ));
                }
                args.url = token.to_string();
            }
            unknown => {
                return Err(format!(
                    "Unknown flag: '{}'. Run 'help' for usage.",
                    unknown
                ));
            }
        }
        i += 1;
    }

    if args.url.is_empty() {
        return Err("No URL provided.\n  Usage: download [flags] <url>".to_string());
    }

    Ok(args)
}

// ─────────────────────────────────────────────────────────────────────────────
// Download dispatcher
// ─────────────────────────────────────────────────────────────────────────────

fn run_download(args: DownloadArgs, config: &Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut job = Job::new(args.url);
    job.output = args.output;
    job.quiet = args.quiet;
    job.advanced = args.advanced;
    job.forced = args.forced;
    job.sha256 = args.sha256;
    job.headers = crate::parse_extra_headers(&args.headers);
    job.resume = args.resume;
    job.quality = VideoQuality::parse(&args.quality);

    let ctx = Ctx {
        config: config.clone(),
        cancel: Arc::new(AtomicBool::new(false)),
    };
    crate::run_and_record(&job, &ctx).map(|_| ())
}

// ─────────────────────────────────────────────────────────────────────────────
// Config sub-commands
// ─────────────────────────────────────────────────────────────────────────────

fn cmd_config(parts: &[&str], config: &mut Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let subcmd = parts.first().copied().unwrap_or("show");
    match subcmd {
        "show" => {
            println!("Current configuration:");
            println!("  connections   {}", config.optimization.max_connections);
            println!(
                "  speed-limit   {}",
                match config.optimization.speed_limit {
                    Some(s) => format!("{} B/s", s),
                    None => "unlimited".to_string(),
                }
            );
            println!(
                "  ytdlp-path    {}",
                config.ytdlp_path.as_deref().unwrap_or("auto-detect")
            );
            println!(
                "  proxy         {}",
                if config.proxy.enabled {
                    config
                        .proxy
                        .url
                        .as_deref()
                        .unwrap_or("(enabled but no URL set)")
                        .to_string()
                } else {
                    "disabled".to_string()
                }
            );
        }
        "set" => {
            if parts.len() < 3 {
                println!("Usage: config set <key> <value>");
                println!("  keys: connections, speed-limit, ytdlp-path");
                return Ok(());
            }
            let key = parts[1];
            let value = parts[2];
            match key {
                "connections" => {
                    let n: usize = value
                        .parse()
                        .map_err(|_| format!("'{}' is not a valid number", value))?;
                    config.optimization.max_connections = n.clamp(1, 32);
                    config.save()?;
                    println!(
                        "Max connections set to {}",
                        config.optimization.max_connections
                    );
                }
                "speed-limit" => {
                    if value == "0" || value == "unlimited" {
                        config.optimization.speed_limit = None;
                        println!("Speed limit removed (unlimited)");
                    } else {
                        let n: u64 = value
                            .parse()
                            .map_err(|_| format!("'{}' is not a valid number of bytes", value))?;
                        config.optimization.speed_limit = Some(n);
                        println!("Speed limit set to {} B/s", n);
                    }
                    config.save()?;
                }
                "ytdlp-path" => {
                    if value == "auto" || value.is_empty() {
                        config.ytdlp_path = None;
                        println!("yt-dlp path: auto-detect");
                    } else {
                        config.ytdlp_path = Some(utils::expand_tilde(value));
                        println!("yt-dlp path set to {}", utils::expand_tilde(value));
                    }
                    config.save()?;
                }
                "compression" | "cache" => {
                    println!(
                        "'{}' was removed in 1.7.1 (it never affected downloads).",
                        key
                    );
                }
                unknown => {
                    println!(
                        "Unknown config key: '{}'\n  Available keys: connections, speed-limit, ytdlp-path",
                        unknown
                    );
                }
            }
        }
        _ => println!("Usage: config [show | set <key> <value>]"),
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Help text
// ─────────────────────────────────────────────────────────────────────────────

fn print_help() {
    println!(
        r#"
Commands
────────
  download [flags] <url>     Download a file

    Flags:
      -a, --advanced         Turbo mode: parallel connections, resumable
          --turbo            Alias for --advanced
      -o, --output <path>    Save to this file or directory
      -q, --quiet            No progress output
      -c, --continue         Resume a partial download (<file>.part)
      -H, --header <h>       Extra HTTP header, e.g. -H "Referer: https://x"
          --sha256 <hash>    Verify SHA-256 after download
          --quality <q>      yt-dlp quality: best, 1080p, 720p, 480p, 360p, audio
          --ftp / --sftp / --webdav / --ytdlp
                             Force a protocol (normally detected from the URL)
          --torrent          Force torrent/magnet handling
          --metalink         Treat source as a Metalink manifest (.meta4/.metalink)

    Quote paths with spaces: download -o "~/My Downloads/x.iso" <url>
    You can also paste a URL on its own to download it.

  history                    Show recent download history (last 50)
  history clear              Remove all history entries
  history clear completed    Remove completed, cancelled and handed-off entries

  config                     Show current configuration
  config show                Show current configuration
  config set <key> <value>   Update a setting
    keys:
      connections  <1-32>           Parallel HTTP connections
      speed-limit  <bytes/s | 0>   Bandwidth cap (0 = unlimited)
      ytdlp-path   <path | auto>    yt-dlp binary to use

  clear                      Clear the terminal screen
  version                    Show KGet version
  help                       Show this help
  exit / quit                Exit interactive mode

Examples
────────
  download https://example.com/file.zip
  download -a -o ~/Downloads/ubuntu.iso https://releases.ubuntu.com/...
  download --sha256 abc123 https://example.com/checksummed.tar.gz
  download ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz
  https://www.youtube.com/watch?v=...
  download --sftp sftp://user@server.com/backups/db.sql.gz
  download magnet:?xt=urn:btih:...
  download --metalink ubuntu.meta4
  download --metalink https://releases.ubuntu.com/ubuntu.meta4
  history
  history clear completed
  config set connections 8
  config set speed-limit 1048576
  config set speed-limit 0
"#
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

pub fn interactive_mode() {
    println!("{}", BANNER);
    println!("  Fast, Multi-Protocol Download Manager");
    println!("  v{}  —  Interactive Mode", env!("CARGO_PKG_VERSION"));
    println!("  Type 'help' for commands, 'exit' to quit.\n");

    let mut config = Config::load().unwrap_or_else(|e| {
        eprintln!("Warning: could not load config ({}). Using defaults.", e);
        Config::default()
    });

    let mut rl = match DefaultEditor::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to initialise line editor: {}", e);
            return;
        }
    };

    loop {
        let readline = rl.readline("kget> ");
        match readline {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(input);

                let owned: Vec<String> = match shlex::split(input) {
                    Some(p) if !p.is_empty() => p,
                    Some(_) => continue,
                    None => {
                        eprintln!("Error: unbalanced quotes");
                        continue;
                    }
                };
                let parts: Vec<&str> = owned.iter().map(String::as_str).collect();
                let cmd = parts[0];

                // A URL pasted on its own starts a download.
                if parts.len() == 1 && (cmd.contains("://") || cmd.starts_with("magnet:?")) {
                    match parse_download_args(&parts) {
                        Ok(args) => {
                            if let Err(e) = run_download(args, &config) {
                                eprintln!("Download failed: {}", e);
                            }
                        }
                        Err(e) => eprintln!("Error: {}", e),
                    }
                    continue;
                }

                match cmd {
                    "exit" | "quit" => {
                        println!("Goodbye!");
                        break;
                    }
                    "help" | "?" => print_help(),
                    "version" => println!("KGet v{}", env!("CARGO_PKG_VERSION")),
                    "clear" => {
                        // ANSI escape: clear screen, move cursor to top-left
                        print!("\x1B[2J\x1B[1;1H");
                        let _ = {
                            use std::io::Write;
                            std::io::stdout().flush()
                        };
                    }
                    "history" => {
                        let sub = parts.get(1).copied().unwrap_or("");
                        match sub {
                            "clear" => {
                                let scope = parts.get(2).copied().unwrap_or("all");
                                let mut removed = 0usize;
                                let r = DownloadHistory::update(|entries| {
                                    let before = entries.len();
                                    match scope {
                                        "completed" | "done" => entries.retain(|e| {
                                            e.status == kget::queue::EntryStatus::Failed
                                        }),
                                        _ => entries.clear(),
                                    }
                                    removed = before - entries.len();
                                });
                                match r {
                                    Err(e) => eprintln!("Failed to save history: {}", e),
                                    Ok(()) => println!("Removed {} history entries.", removed),
                                }
                            }
                            _ => {
                                let history = DownloadHistory::load();
                                let entries = history.recent(50);
                                if entries.is_empty() {
                                    println!("No download history.");
                                } else {
                                    println!(
                                        "{:<16} {:<22} {:<12} File",
                                        "ID", "Date (UTC)", "Status"
                                    );
                                    println!("{}", "-".repeat(86));
                                    for e in entries {
                                        println!(
                                            "{:<16} {:<22} {:<12} {}",
                                            e.id,
                                            e.created_at_display(),
                                            e.status,
                                            e.filename
                                        );
                                    }
                                }
                            }
                        }
                    }
                    "config" => {
                        let sub = &parts[1..];
                        if let Err(e) = cmd_config(sub, &mut config) {
                            eprintln!("Config error: {}", e);
                        }
                    }
                    "download" | "get" | "dl" => {
                        let dl_parts = &parts[1..];
                        if dl_parts.is_empty() {
                            eprintln!("Error: No URL provided.\n  Usage: download [flags] <url>");
                            continue;
                        }
                        match parse_download_args(dl_parts) {
                            Ok(args) => {
                                if let Err(e) = run_download(args, &config) {
                                    eprintln!("Download failed: {}", e);
                                }
                            }
                            Err(e) => eprintln!("Error: {}", e),
                        }
                    }
                    _ => eprintln!(
                        "Unknown command: '{}'. Type 'help' to see available commands.",
                        cmd
                    ),
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                println!("Goodbye!");
                break;
            }
            Err(err) => {
                eprintln!("Readline error: {:?}", err);
                break;
            }
        }
    }
}
