use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransmissionSettings {
    pub host: String,
    pub port: u16,
    pub rpc_path: String,
    pub web_path: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Default for TransmissionSettings {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 9091,
            rpc_path: "/transmission/rpc".to_string(),
            web_path: "/transmission/web/".to_string(),
            username: None,
            password: None,
        }
    }
}

impl TransmissionSettings {
    pub fn rpc_url(&self) -> String {
        let path = if self.rpc_path.starts_with('/') { self.rpc_path.clone() } else { format!("/{}", self.rpc_path) };
        format!("http://{}:{}{}", self.host, self.port, path)
    }

    pub fn web_url(&self) -> String {
        let path = if self.web_path.starts_with('/') { self.web_path.clone() } else { format!("/{}", self.web_path) };
        format!("http://{}:{}{}", self.host, self.port, path)
    }

    fn apply_url(&mut self, url: &str, default_path: &str, is_web: bool) {
        // Parse simples sem depender de crate extra: aceita "http://host:port/path"
        // Se falhar, ignora e mantém defaults
        if let Ok(u) = url::Url::parse(url) {
            if let Some(h) = u.host_str() {
                self.host = h.to_string();
            }
            if let Some(p) = u.port() {
                self.port = p;
            }
            let path = u.path();
            if !path.is_empty() && path != "/" {
                if is_web { self.web_path = path.to_string(); } else { self.rpc_path = path.to_string(); }
            } else {
                if is_web { self.web_path = default_path.to_string(); } else { self.rpc_path = default_path.to_string(); }
            }
        }
    }

    pub fn from_env() -> Self {
        let mut s = Self::default();

        // Compatibilidade (URL completa)
        if let Ok(v) = std::env::var("KGET_TRANSMISSION_URL") {
            if !v.trim().is_empty() {
                s.apply_url(&v, "/transmission/rpc", false);
            }
        }
        if let Ok(v) = std::env::var("KGET_TRANSMISSION_WEB") {
            if !v.trim().is_empty() {
                s.apply_url(&v, "/transmission/web/", true);
            }
        }

        // Preferir vars novas quando existirem
        if let Ok(v) = std::env::var("KGET_TRANSMISSION_HOST") {
            if !v.trim().is_empty() {
                s.host = v;
            }
        }
        if let Ok(v) = std::env::var("KGET_TRANSMISSION_PORT") {
            if let Ok(p) = v.parse::<u16>() {
                s.port = p;
            }
        }
        if let Ok(v) = std::env::var("KGET_TRANSMISSION_RPC_PATH") {
            if !v.trim().is_empty() {
                s.rpc_path = v;
            }
        }
        if let Ok(v) = std::env::var("KGET_TRANSMISSION_WEB_PATH") {
            if !v.trim().is_empty() {
                s.web_path = v;
            }
        }

        s.username = std::env::var("KGET_TRANSMISSION_USER").ok().filter(|x| !x.is_empty());
        s.password = std::env::var("KGET_TRANSMISSION_PASS").ok().filter(|x| !x.is_empty());

        s
    }
}