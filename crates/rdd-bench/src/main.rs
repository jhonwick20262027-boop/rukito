//! Arnés de benchmarks HTTP y WebSocket de rdd (§10.10, RF-252).
//!
//! Levanta un `AxumPlatform` real en un puerto efímero con tres rutas HTTP y un
//! canal de eco, mide latencias y tasas, y escribe
//! `docs/assets/benchmarks.json` para la página del menú Benchmark.
//!
//! ```text
//! cargo run --release -p rdd-bench
//! RDD_BENCH_HTTP_MS=3000 RDD_BENCH_WS_MESSAGES=2000 cargo run --release -p rdd-bench
//! ```

mod http;
mod report;
mod server;
mod stats;
mod ws;

use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let http_duration_ms = env_u64("RDD_BENCH_HTTP_MS", 1500);
    let ws_messages = env_usize("RDD_BENCH_WS_MESSAGES", 1200);
    let warmup = env_usize("RDD_BENCH_WARMUP", 20);

    println!("rdd-bench: levantando el servidor de medición…");
    let running = server::start().await?;
    println!("rdd-bench: servidor en http://{}", running.addr);

    println!(
        "rdd-bench: HTTP · {} escenarios, concurrencia {:?}, {} ms",
        http::SCENARIOS.len(),
        http::CONCURRENCIES,
        http_duration_ms
    );
    let http_results = http::run(
        running.addr,
        Duration::from_millis(http_duration_ms),
        warmup,
    )
    .await;

    println!(
        "rdd-bench: WebSocket · {} tamaños, concurrencia {:?}, {} mensajes",
        ws::SCENARIOS.len(),
        ws::CONCURRENCIES,
        ws_messages
    );
    let ws_results = ws::run(running.addr, ws_messages).await;

    running.stop().await;

    let config = report::Config {
        http_duration_ms,
        http_warmup: warmup,
        ws_messages,
    };
    let report = report::build(&config, &http_results, &ws_results);
    let path = report::write(&report)?;

    print_summary(&http_results, &ws_results);
    println!("rdd-bench: informe escrito en {}", path.display());
    Ok(())
}

fn print_summary(http_results: &[http::ScenarioResult], ws_results: &[ws::ScenarioResult]) {
    println!("\nHTTP");
    for scenario in http_results {
        for result in &scenario.results {
            println!(
                "  {:<24} c={:<3} {:>9.0} req/s  p50 {:>7.2} ms  p95 {:>7.2} ms  p99 {:>7.2} ms",
                scenario.scenario.name,
                result.concurrency,
                result.rps,
                result.p50_ms,
                result.p95_ms,
                result.p99_ms
            );
        }
    }
    println!("\nWebSocket");
    for scenario in ws_results {
        for result in &scenario.results {
            println!(
                "  {:<20} c={:<3} {:>9.0} msg/s  p50 {:>7.2} ms  p95 {:>7.2} ms  p99 {:>7.2} ms",
                scenario.scenario.name,
                result.concurrency,
                result.messages_per_sec,
                result.p50_ms,
                result.p95_ms,
                result.p99_ms
            );
        }
    }
    println!();
}

fn env_u64(key: &str, fallback: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn env_usize(key: &str, fallback: usize) -> usize {
    env_u64(key, fallback as u64) as usize
}
