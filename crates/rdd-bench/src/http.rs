//! Benchmarks HTTP: conexiones keep-alive por worker y latencia por petición.
//!
//! Cada worker mantiene su propia conexión HTTP/1.1, así que la tasa mide el
//! servidor y no el costo del handshake. La duración se fija por reloj para que
//! todos los niveles de concurrencia comparen la misma ventana.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use tokio::net::TcpStream;

use crate::stats;

#[derive(Clone, Copy)]
pub struct Scenario {
    pub id: &'static str,
    pub name: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub body: Option<&'static [u8]>,
}

pub const SCENARIOS: [Scenario; 3] = [
    Scenario {
        id: "ping",
        name: "GET /bench/ping",
        method: "GET",
        path: "/bench/ping",
        body: None,
    },
    Scenario {
        id: "items",
        name: "GET /bench/items/:id",
        method: "GET",
        path: "/bench/items/42",
        body: None,
    },
    Scenario {
        id: "echo",
        name: "POST /bench/echo",
        method: "POST",
        path: "/bench/echo",
        body: Some(crate::server::ECHO_BODY),
    },
];

pub const CONCURRENCIES: [usize; 3] = [1, 8, 32];

#[derive(Debug, Clone, Serialize)]
pub struct Result {
    pub concurrency: usize,
    pub requests: u64,
    pub errors: u64,
    pub rps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

pub struct ScenarioResult {
    pub scenario: Scenario,
    pub results: Vec<Result>,
}

pub async fn run(addr: SocketAddr, duration: Duration, warmup: usize) -> Vec<ScenarioResult> {
    let mut out = Vec::new();
    for scenario in SCENARIOS {
        let mut results = Vec::new();
        for concurrency in CONCURRENCIES {
            let workers: Vec<_> = (0..concurrency)
                .map(|_| tokio::spawn(worker(addr, scenario, duration, warmup)))
                .collect();
            let started = Instant::now();
            let mut samples = Vec::new();
            let mut errors = 0;
            for worker in workers {
                let (mut latencies, failed) = worker.await.expect("el worker HTTP entró en pánico");
                samples.append(&mut latencies);
                errors += failed;
            }
            let elapsed = started.elapsed();
            let requests = samples.len() as u64 + errors;
            results.push(Result {
                concurrency,
                requests,
                errors,
                rps: stats::rate(samples.len() as u64, elapsed),
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

async fn worker(
    addr: SocketAddr,
    scenario: Scenario,
    duration: Duration,
    warmup: usize,
) -> (Vec<u64>, u64) {
    let mut latencies = Vec::new();
    let mut sender = match connect(addr).await {
        Some(sender) => sender,
        None => return (latencies, 1),
    };
    for _ in 0..warmup {
        if request(&mut sender, scenario).await.is_none() {
            return (latencies, 1);
        }
    }
    let deadline = Instant::now() + duration;
    let mut errors = 0;
    while Instant::now() < deadline {
        match request(&mut sender, scenario).await {
            Some(nanos) => latencies.push(nanos),
            None => {
                errors += 1;
                break;
            }
        }
    }
    (latencies, errors)
}

async fn connect(addr: SocketAddr) -> Option<hyper::client::conn::http1::SendRequest<Full<Bytes>>> {
    let stream = TcpStream::connect(addr).await.ok()?;
    let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .ok()?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    Some(sender)
}

async fn request(
    sender: &mut hyper::client::conn::http1::SendRequest<Full<Bytes>>,
    scenario: Scenario,
) -> Option<u64> {
    let body = scenario
        .body
        .map(|body| Full::new(Bytes::from_static(body)))
        .unwrap_or_else(|| Full::new(Bytes::new()));
    let req = http::Request::builder()
        .method(scenario.method)
        .uri(scenario.path)
        .header(http::header::HOST, "127.0.0.1")
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(body)
        .ok()?;
    let started = Instant::now();
    let resp = sender.send_request(req).await.ok()?;
    let (parts, body) = resp.into_parts();
    let body = body.collect().await.ok()?.to_bytes();
    let nanos = started.elapsed().as_nanos() as u64;
    if parts.status.is_success() && !body.is_empty() {
        Some(nanos)
    } else {
        None
    }
}
