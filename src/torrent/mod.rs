use std::error::Error;
use std::sync::Arc;

use crate::config::ProxyConfig;
use crate::optimization::Optimizer;

mod external;
mod settings;

#[cfg(feature = "torrent-transmission")]
mod transmission;

pub type StatusCb = Arc<dyn Fn(String) + Send + Sync>;
pub type ProgressCb = Arc<dyn Fn(f32) + Send + Sync>;

#[derive(Default, Clone)]
pub struct TorrentCallbacks {
    pub status: Option<StatusCb>,
    pub progress: Option<ProgressCb>,
}

fn emit_status(cb: &TorrentCallbacks, msg: impl Into<String>) {
    if let Some(f) = &cb.status {
        f(msg.into());
    }
}

fn emit_progress(cb: &TorrentCallbacks, p: f32) {
    if let Some(f) = &cb.progress {
        f(p.clamp(0.0, 1.0));
    }
}

/// Backend selection:
/// - default: "external" (abre no cliente instalado)
/// - "transmission": usa Transmission RPC (requer feature torrent-transmission)
fn selected_backend() -> String {
    std::env::var("KGET_TORRENT_BACKEND")
        .unwrap_or_else(|_| "external".to_string())
        .to_lowercase()
}

pub fn download_magnet(
    magnet: &str,
    output_dir: &str,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    cb: TorrentCallbacks,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    emit_progress(&cb, 0.0);

    match selected_backend().as_str() {
        "transmission" => {
            #[cfg(feature = "torrent-transmission")]
            {
                return transmission::download_via_transmission(
                    magnet,
                    output_dir,
                    quiet,
                    proxy,
                    optimizer,
                    cb,
                );
            }

            #[cfg(not(feature = "torrent-transmission"))]
            {
                emit_status(
                    &cb,
                    "Torrent backend 'transmission' not available (compile with --features torrent-transmission). Falling back to external client.",
                );
            }
        }
        _ => {}
    }

    emit_status(
        &cb,
        format!(
            "Opening magnet link in your default torrent client (output folder may be managed by that client): {}",
            magnet
        ),
    );

    external::open_magnet_in_default_client(magnet)?;
    emit_progress(&cb, 1.0);
    Ok(())
}
