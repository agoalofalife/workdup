use anyhow::{Context, Result};
use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::Deserialize;
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Deserialize)]
pub struct MainConfig {
    #[serde(default)]
    pub defaults: Defaults,

    #[serde(default)]
    pub http: Http,

    #[serde(default)]
    pub namespaces: Vec<Namespace>,
}

#[derive(Debug, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_scan", with = "humantime_serde")]
    pub scan_interval: Duration,

    #[serde(default = "default_cleanup", with = "humantime_serde")]
    pub cleanup_interval: Duration,

    #[serde(default = "default_query")]
    pub query: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            scan_interval: default_scan(),
            cleanup_interval: default_cleanup(),
            query: default_query(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Namespace {
    pub name: String,
    pub host: String,
    pub api_key: Option<String>,
    pub tls: Option<Tls>,
    // overrides: None → берётся из defaults
    #[serde(default, with = "humantime_serde::option")]
    pub scan_interval: Option<Duration>,
    #[serde(default, with = "humantime_serde::option")]
    pub cleanup_interval: Option<Duration>,
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Tls {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub ca_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Http {
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for Http {
    fn default() -> Self {
        Self {
            port: default_port(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct ResolvedNamespace {
    pub name: String,
    pub host: String,
    pub api_key: Option<String>,
    pub tls: Option<Tls>,
    pub scan_interval: Duration,
    pub cleanup_interval: Duration,
    pub query: String,
}

fn default_scan() -> Duration {
    Duration::from_secs(3600) // 1 hour
}

fn default_cleanup() -> Duration {
    Duration::from_secs(86_400) // 1 day
}
fn default_query() -> String {
    "ExecutionStatus = 'Running'".into()
}

fn default_port() -> u16 {
    8000
}

pub fn load(config_path: &std::path::Path) -> Result<MainConfig> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("read config {}", config_path.display()))?;

    let expanded = shellexpand::env(&raw)
        .map_err(|e| anyhow::anyhow!("env var {} was not defined in config", e.var_name))?;

    let cfg = Figment::new()
        .merge(Toml::string(&expanded))
        .merge(Env::prefixed("WORKDUP__").split("__"))
        .extract()
        .context("parse config")?;

    Ok(cfg)
}

impl MainConfig {
    pub fn resolve_namespace_section(&self) -> Vec<ResolvedNamespace> {
        let default = &self.defaults;

        self.namespaces
            .iter()
            .map(|ns| ResolvedNamespace {
                name: ns.name.clone(),
                host: ns.host.clone(),
                api_key: ns.api_key.clone(),
                tls: ns.tls.clone(),
                scan_interval: ns.scan_interval.unwrap_or(default.scan_interval),
                cleanup_interval: ns.cleanup_interval.unwrap_or(default.cleanup_interval),
                query: ns.query.clone().unwrap_or_else(|| default.query.clone()),
            })
            .collect()
    }
}

pub fn validate(namespaces: &[ResolvedNamespace]) -> Result<()> {
    anyhow::ensure!(!namespaces.is_empty(), "no one namespace was defined");

    let mut seen = std::collections::HashSet::new();
    for ns in namespaces {
        anyhow::ensure!(seen.insert(&ns.name), "namespace '{}' dublicate", ns.name);
        anyhow::ensure!(!ns.host.is_empty(), "namespace '{}': empty host", ns.name);

        if let Some(tls) = &ns.tls {
            for p in [&tls.cert_path, &tls.key_path]
                .into_iter()
                .chain(tls.ca_path.as_ref())
            {
                anyhow::ensure!(
                    p.is_file(),
                    "namespace '{}': file not defined: {}",
                    ns.name,
                    p.display()
                );
            }
        }
    }
    Ok(())
}
