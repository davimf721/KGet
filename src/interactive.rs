use kget::advanced_download::AdvancedDownloader;
use kget::config::Config;
use kget::download::download as http_download;
use kget::ftp::FtpDownloader;
use kget::metalink;
use kget::optimization::Optimizer;
use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};
use kget::sftp::SftpDownloader;
use kget::utils;
use kget::DownloadOptions;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::error::Error;

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
    ftp: bool,
    sftp: bool,
    torrent: bool,
    metalink: bool,
    sha256: Option<String>,
}

/// Parse flags and URL from the tokens after the `download` command.
///
/// Accepted flags:
/// ```text
/// -a / --advanced / --turbo     parallel multi-connection download
/// -o / --output <path>          destination file or directory
/// -q / --quiet                  suppress progress output
/// --sha256 <hash>               verify file hash after download
/// --ftp                         force FTP protocol
/// --sftp                        force SFTP protocol
/// --torrent                     force torrent/magnet handling
/// --metalink                    treat source as a Metalink manifest
/// ```
fn parse_download_args(parts: &[&str]) -> Result<DownloadArgs, String> {
    let mut args = DownloadArgs {
        url: String::new(),
        output: None,
        quiet: false,
        advanced: false,
        ftp: false,
        sftp: false,
        torrent: false,
        metalink: false,
        sha256: None,
    };

    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            "-a" | "--advanced" | "--turbo" => args.advanced = true,
            "-q" | "--quiet" => args.quiet = true,
            "--ftp" => args.ftp = true,
            "--sftp" => args.sftp = true,
            "--torrent" => args.torrent = true,
            "--metalink" => args.metalink = true,
            "-o" | "--output" => {
                i += 1;
                if i >= parts.len() {
                    return Err(format!("'{}' requires a path argument", parts[i - 1]));
                }
                args.output = Some(parts[i].to_string());
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
            unknown => return Err(format!("Unknown flag: '{}'. Run 'help' for usage.", unknown)),
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
    let optimizer = Optimizer::from_config(config.optimization.clone());

    let is_metalink = args.metalink || metalink::is_metalink(&args.url);

    if is_metalink {
        let output_dir = args.output.as_deref().unwrap_or(".");
        let result = metalink::download_metalink(
            &args.url,
            output_dir,
            args.quiet,
            config.proxy.clone(),
            optimizer,
        );
        // Record to history
        let mut history = DownloadHistory::load();
        let entry = HistoryEntry::new(&args.url, output_dir, None);
        let (status, err) = match &result {
            Ok(()) => (EntryStatus::Completed, None),
            Err(e) => (EntryStatus::Failed, Some(e.to_string())),
        };
        history.record(entry, status, err);
        let _ = history.save();
        return result;
    }

    // Capture url/output_dir before consuming args, for history recording
    let url_for_history = args.url.clone();
    let output_dir_for_history = args.output.as_deref().unwrap_or(".").to_string();
    let sha256_for_history = args.sha256.clone();

    let result: Result<(), Box<dyn Error + Send + Sync>> =
        // Auto-detect magnet links as torrent downloads
        if args.torrent || args.url.starts_with("magnet:?") {
            let output_dir = args.output.unwrap_or_else(|| "torrent_output".to_string());
            kget::torrent::download_magnet(
                &args.url,
                &output_dir,
                args.quiet,
                config.proxy.clone(),
                optimizer,
                kget::torrent::TorrentCallbacks::default(),
            )
        } else if args.ftp {
            let output = utils::resolve_output_path(args.output, &args.url, "ftp_output");
            FtpDownloader::new(args.url, output, args.quiet, config.proxy.clone(), optimizer)
                .download()
        } else if args.sftp {
            let output = utils::resolve_output_path(args.output, &args.url, "sftp_output");
            SftpDownloader::new(args.url, output, args.quiet, config.proxy.clone(), optimizer)
                .download()
        } else if args.advanced {
            let output = utils::resolve_output_path(args.output, &args.url, "download");
            let mut dl = AdvancedDownloader::new(
                args.url,
                output,
                args.quiet,
                config.proxy.clone(),
                optimizer,
            );
            if let Some(hash) = args.sha256 {
                dl.set_expected_sha256(hash);
            }
            dl.download()
        } else {
            // Default: simple HTTP/HTTPS
            let options = DownloadOptions {
                quiet_mode: args.quiet,
                output_path: args.output,
                verify_iso: args.sha256.is_some(),
                expected_sha256: args.sha256,
            };
            http_download(&args.url, config.proxy.clone(), optimizer, options, None)
        };

    // Record to history (best-effort)
    let mut history = DownloadHistory::load();
    let entry = HistoryEntry::new(&url_for_history, &output_dir_for_history, sha256_for_history.as_deref());
    let (status, err) = match &result {
        Ok(()) => (EntryStatus::Completed, None),
        Err(e) => (EntryStatus::Failed, Some(e.to_string())),
    };
    history.record(entry, status, err);
    let _ = history.save();

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// Config sub-commands
// ─────────────────────────────────────────────────────────────────────────────

fn cmd_config(parts: &[&str], config: &mut Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let subcmd = parts.first().copied().unwrap_or("show");
    match subcmd {
        "show" => {
            println!("Current configuration:");
            println!(
                "  connections   {}",
                config.optimization.max_connections
            );
            println!(
                "  speed-limit   {}",
                match config.optimization.speed_limit {
                    Some(s) => format!("{} B/s", s),
                    None => "unlimited".to_string(),
                }
            );
            println!("  compression   {}", config.optimization.compression);
            println!("  cache         {}", config.optimization.cache_enabled);
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
                println!("  keys: connections, speed-limit, compression, cache");
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
                    println!("Max connections set to {}", config.optimization.max_connections);
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
                "compression" => {
                    let v: bool = value
                        .parse()
                        .map_err(|_| format!("'{}' is not 'true' or 'false'", value))?;
                    config.optimization.compression = v;
                    config.save()?;
                    println!("Compression set to {}", v);
                }
                "cache" => {
                    let v: bool = value
                        .parse()
                        .map_err(|_| format!("'{}' is not 'true' or 'false'", value))?;
                    config.optimization.cache_enabled = v;
                    config.save()?;
                    println!("Cache set to {}", v);
                }
                unknown => {
                    println!(
                        "Unknown config key: '{}'\n  Available keys: connections, speed-limit, compression, cache",
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
          --sha256 <hash>    Verify SHA-256 after download
          --ftp              Use FTP protocol  (ftp://[user:pass@]host/path)
          --sftp             Use SFTP protocol (sftp://[user[:pass]@]host/path)
          --torrent          Force torrent/magnet handling
          --metalink         Treat source as a Metalink manifest (.meta4/.metalink)

  history                    Show recent download history (last 50)
  history clear              Remove all history entries
  history clear completed    Remove only completed/cancelled entries

  config                     Show current configuration
  config show                Show current configuration
  config set <key> <value>   Update a setting
    keys:
      connections  <1-32>           Parallel HTTP connections
      speed-limit  <bytes/s | 0>   Bandwidth cap (0 = unlimited)
      compression  <true|false>     Enable compression cache
      cache        <true|false>     Enable local download cache

  clear                      Clear the terminal screen
  version                    Show KGet version
  help                       Show this help
  exit / quit                Exit interactive mode

Examples
────────
  download https://example.com/file.zip
  download -a -o ~/Downloads/ubuntu.iso https://releases.ubuntu.com/...
  download --sha256 abc123 https://example.com/checksummed.tar.gz
  download --ftp ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz
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

                let parts: Vec<&str> = input.split_whitespace().collect();
                let cmd = parts[0];

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
                                let mut history = DownloadHistory::load();
                                let n = match scope {
                                    "completed" | "done" => history.clear_completed(),
                                    _ => history.clear_all(),
                                };
                                if let Err(e) = history.save() {
                                    eprintln!("Failed to save history: {}", e);
                                } else {
                                    println!("Removed {} history entries.", n);
                                }
                            }
                            _ => {
                                let history = DownloadHistory::load();
                                let entries = history.recent(50);
                                if entries.is_empty() {
                                    println!("No download history.");
                                } else {
                                    println!("{:<10} {:<22} {:<12} {}", "ID", "Date (UTC)", "Status", "File");
                                    println!("{}", "-".repeat(80));
                                    for e in entries {
                                        println!(
                                            "{:<10} {:<22} {:<12} {}",
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
