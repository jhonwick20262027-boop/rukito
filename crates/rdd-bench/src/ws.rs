//! Benchmark WebSocket: eco de ida y vuelta sobre el transporte de `rdd-axum`.
//!
//! Cada conexión envía mensajes de forma secuencial y espera el eco antes de
//! medir el siguiente, así el tiempo es el viaje completo cliente → servidor →
//! cliente y no una estimación.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tokio_tungstenite::tungstenite::Message;

use crate::stats;

pub struct Scenario {
    pub id: &'static str,
    pub name: &'static str,
    pub bytes: usize,
}

pub const SCENARIOS: [Scenario; 3] = [
    Scenario {
        id: "ws-64",
        name: "WS eco · 64 B",
        bytes: 64,
    },
    Scenario {
        id: "ws-1024",
        name: "WS eco · 1 KB",
        bytes: 1024,
    },
    Scenario {
        id: "ws-8192",
        name: "WS eco · 8 KB",
        bytes: 8192,
    },
];

pub const CONCURRENCIES: [usize; 2] = [1, 8];

#[derive(Debug, Clone, Serialize)]
pub struct Result {
    pub concurrency: usize,
    pub messages: u64,
    pub errors: u64,
    pub messages_per_sec: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

pub struct ScenarioResult {
    pub scenario: Scenario,
    pub results: Vec<Result>,
}

pub async fn run(addr: SocketAddr, messages: usize) -> Vec<ScenarioResult> {
    let mut out = Vec::new();
    for scenario in SCENARIOS {
        let payload = "x".repeat(scenario.bytes);
        let mut results = Vec::new();
        for concurrency in CONCURRENCIES {
            let workers: Vec<_> = (0..concurrency)
                .map(|_| {
                    let payload = payload.clone();
                    tokio::spawn(worker(addr, payload, messages))
                })
                .collect();
            let started = Instant::now();
            let mut samples = Vec::new();
            let mut errors = 0;
            for worker in workers {
                let (mut latencies, failed) = worker.await.expect("el worker WS entró en pánico");
                samples.append(&mut latencies);
                errors += failed;
            }
            let elapsed = started.elapsed();
            results.push(Result {
                concurrency,
                messages: samples.len() as u64,
                errors,
                messages_per_sec: stats::rate(samples.len() as u64, elapsed),
                p50_ms: stats::nanos_to_ms(stats::percentile(&samples, 50.0)),
                p95_ms: stats::nanos_to_ms(stats::percentile(&samples, 95.0)),
                p99_ms: stats::nanos_to_ms(stats::percentile(&samples, 99.0)),
                max_ms: stats::nanos_to_ms(samples.iter().copied().max().unwrap_or(0)),
            });
        }
        out.push(ScenarioResult { scenario, results });
    }
    out
}

async fn worker(addr: SocketAddr, payload: String, messages: usize) -> (Vec<u64>, u64) {
    let url = format!("ws://{addr}/ws/echo");
    let socket = tokio_tungstenite::connect_async(&url).await;
    let (mut socket, _) = match socket {
        Ok(pair) => pair,
        Err(_) => return (Vec::new(), 1),
    };

    let mut latencies = Vec::with_capacity(messages);
    let mut errors = 0;
    for _ in 0..messages {
        let started = Instant::now();
        if socket
            .send(Message::Text(payload.clone().into()))
            .await
            .is_err()
        {
            errors += 1;
            break;
        }
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) => {
                    latencies.push(started.elapsed().as_nanos() as u64);
                    break;
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                    errors += 1;
                    break;
                }
                Some(Ok(_)) => {}
            }
        }
        if errors > 0 {
            break;
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), socket.close(None)).await;
    (latencies, errors)
}
