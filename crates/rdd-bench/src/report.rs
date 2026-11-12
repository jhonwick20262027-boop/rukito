//! Informe JSON consumido por la página `docs/benchmark.html`.
//!
//! El esquema es estable: la página no adivina campos ni calcula métricas, solo
//! dibuja lo que este archivo publica (RF-252).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::{http, ws};

pub struct Config {
    pub http_duration_ms: u64,
    pub http_warmup: usize,
    pub ws_messages: usize,
}

pub fn build(
    config: &Config,
    http_results: &[http::ScenarioResult],
    ws_results: &[ws::ScenarioResult],
) -> Value {
    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);

    let http_scenarios: Vec<Value> = http_results
        .iter()
        .map(|scenario| {
            json!({
                "id": scenario.scenario.id,
                "name": scenario.scenario.name,
                "method": scenario.scenario.method,
                "path": scenario.scenario.path,
                "results": scenario.results,
            })
        })
        .collect();

    let ws_scenarios: Vec<Value> = ws_results
        .iter()
        .map(|scenario| {
            json!({
                "id": scenario.scenario.id,
                "name": scenario.scenario.name,
                "bytes": scenario.scenario.bytes,
                "results": scenario.results,
            })
        })
        .collect();

    json!({
        "generatedAt": generated_at,
        "environment": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "cpu": cpu(),
            "rustc": rustc_version(),
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        },
        "config": {
            "httpDurationMs": config.http_duration_ms,
            "httpWarmup": config.http_warmup,
            "wsMessages": config.ws_messages,
            "httpConcurrency": http::CONCURRENCIES,
            "wsConcurrency": ws::CONCURRENCIES,
        },
        "http": http_scenarios,
        "ws": ws_scenarios,
    })
}

pub fn write(value: &Value) -> anyhow::Result<PathBuf> {
    let path = output_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    std::fs::write(&path, format!("{text}\n"))?;
    Ok(path)
}

fn output_path() -> PathBuf {
    match std::env::var("RDD_BENCH_OUT") {
        Ok(path) if !path.trim().is_empty() => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                workspace_root().join(path)
            }
        }
        _ => workspace_root().join("docs/assets/benchmarks.json"),
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

fn cpu() -> String {
    if let Ok(value) = std::env::var("PROCESSOR_IDENTIFIER") {
        return value;
    }
    if let Ok(text) = std::fs::read_to_string("/proc/cpuinfo")
        && let Some(line) = text.lines().find(|line| line.starts_with("model name"))
        && let Some(model) = line.split(':').nth(1)
    {
        return model.trim().to_owned();
    }
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| format!("rust {}", env!("CARGO_PKG_RUST_VERSION")))
}
