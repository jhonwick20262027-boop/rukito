//! V17 (§22): medición del hub con 1000 conexiones a 100 msg/s.
//!
//! Ignorada por defecto: abre 1000 sockets reales y tarda decenas de segundos.
//! Se ejecuta a mano, en release:
//!
//! ```text
//! cargo test -p rdd-platform-tests --release --test carga -- --ignored --nocapture
//! ```
//!
//! El objetivo es fijar `socket_outbound_queue` (§4.4) y el anillo de
//! `rdd-socket` (§10.5): con los valores por defecto (64 y 64) las 1000
//! suscripciones deben recibir el 100 % de los mensajes, sin 1013.

use futures::{SinkExt, StreamExt};
use rdd_axum::AxumPlatform;
use rdd_core::http::{BoxFuture, HttpApp, Platform, ShutdownSignal};
use rdd_core::socket::{SocketConnection, SocketContext, SocketHandler, SocketMessage};
use rdd_core::{App, Result};
use rdd_socket::{Hub, Lagged, Payload};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const TOPIC: &str = "carga";
const CLIENTS: usize = 1000;
const MESSAGES: usize = 500;
const TICK: Duration = Duration::from_millis(10);
const CONNECT_CONCURRENCY: usize = 64;
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Canal de la medición: soltar la suscripción es salir de la sala.
struct LoadFeed {
    hub: Hub,
}

impl SocketHandler for LoadFeed {
    fn authorize(&self, _ctx: &SocketContext) -> Result<()> {
        Ok(())
    }

    fn handle(
        &self,
        _ctx: SocketContext,
        mut conn: Box<dyn SocketConnection>,
    ) -> BoxFuture<'static, Result<()>> {
        let hub = self.hub.clone();
        Box::pin(async move {
            // Suscribir antes del saludo: cuando el cliente recibe `ready`, la
            // sala ya lo cuenta y ninguna difusión posterior se pierde.
            let mut sub = hub.subscribe(TOPIC);
            conn.send(SocketMessage::Text("ready".to_owned())).await?;

            loop {
                tokio::select! {
                    incoming = conn.recv() => match incoming {
                        Some(Ok(SocketMessage::Close(_, _))) | None => break,
                        Some(Ok(_)) => {}
                        Some(Err(err)) => return Err(err),
                    },
                    event = sub.recv() => match event {
                        Ok(Payload::Text(text)) => {
                            if conn.send(SocketMessage::Text(text)).await.is_err() {
                                break;
                            }
                        }
                        Ok(Payload::Json(value)) => {
                            if conn.send(SocketMessage::Text(value.to_string())).await.is_err() {
                                break;
                            }
                        }
                        Ok(Payload::Binary(bytes)) => {
                            if conn.send(SocketMessage::Binary(bytes)).await.is_err() {
                                break;
                            }
                        }
                        // El 1013 es la señal de que el anillo no alcanzó; la
                        // medición lo cuenta como fallo del dimensionado.
                        Err(Lagged(_)) => {
                            conn.close(1013, "cliente lento".to_owned()).await;
                            return Ok(());
                        }
                    },
                }
            }
            Ok(())
        })
    }
}

fn http_app(hub: &Hub) -> HttpApp {
    App::new()
        .without_health()
        .socket("/ws/carga", LoadFeed { hub: hub.clone() })
        .build()
        .expect("la app de la medición debe construirse")
        .http
}

fn start(
    hub: &Hub,
) -> (
    std::net::SocketAddr,
    ShutdownSignal,
    tokio::task::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("puerto efímero");
    listener
        .set_nonblocking(true)
        .expect("listener no bloqueante");
    let addr = listener.local_addr().expect("socket vinculado");
    let listener = TcpListener::from_std(listener).expect("listener de Tokio");
    let app = http_app(hub);
    let shutdown = ShutdownSignal::new();
    let future = Arc::new(AxumPlatform::new())
        .serve(app, listener, shutdown.clone())
        .expect("la plataforma debe arrancar");
    let server = tokio::spawn(async move {
        if let Err(err) = future.await {
            panic!("la plataforma terminó con error: {err}");
        }
    });
    (addr, shutdown, server)
}

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Un cliente de la medición: handshake, saludo y lectura hasta el último
/// mensaje esperado. Cualquier cierre, sobre todo el 1013, se reporta.
struct Outcome {
    received: usize,
    lagged: bool,
    latencies: Vec<u32>,
}

async fn connect(addr: std::net::SocketAddr) -> Client {
    // El backlog del listener es finito: si una conexión se rechaza, se
    // reintenta con una espera creciente en vez de perder el cliente.
    let mut last = None;
    for attempt in 0..30u32 {
        let request = format!("ws://{addr}/ws/carga")
            .into_client_request()
            .expect("petición WS válida");
        match tokio_tungstenite::connect_async(request).await {
            Ok((socket, response)) => {
                assert_eq!(response.status(), 101, "V17: handshake");
                return socket;
            }
            Err(err) => {
                last = Some(err);
                tokio::time::sleep(Duration::from_millis(50 * u64::from(attempt + 1))).await;
            }
        }
    }
    panic!("V17: ningún handshake tras 30 intentos: {last:?}");
}

async fn run_client(
    addr: std::net::SocketAddr,
    permits: Arc<Semaphore>,
    schedule: Arc<OnceLock<Vec<Instant>>>,
    ready: mpsc::Sender<()>,
) -> Outcome {
    // El permiso solo cubre el handshake: sostener la conexión no debe
    // bloquear a los clientes que aún no han nacido.
    let mut ws = {
        let _permit = permits.acquire_owned().await.expect("semáforo vivo");
        connect(addr).await
    };

    loop {
        match tokio::time::timeout(READ_TIMEOUT, ws.next())
            .await
            .unwrap_or_else(|_| panic!("V17: el saludo no llegó en 60s"))
            .unwrap_or_else(|| panic!("V17: canal cerrado antes del saludo"))
            .unwrap_or_else(|err| panic!("V17: error de trama en el saludo: {err}"))
        {
            Message::Text(text) if text.as_str() == "ready" => break,
            Message::Ping(_) | Message::Pong(_) => {}
            other => panic!("V17: saludo inesperado: {other:?}"),
        }
    }
    ready.send(()).await.expect("el receptor sigue vivo");

    let mut received = 0;
    let mut lagged = false;
    let mut latencies = Vec::with_capacity(MESSAGES);
    while received < MESSAGES {
        let frame = match tokio::time::timeout(READ_TIMEOUT, ws.next()).await {
            Err(_) => break,
            Ok(None) => break,
            Ok(Some(Err(err))) => panic!("V17: error de trama al leer: {err}"),
            Ok(Some(Ok(frame))) => frame,
        };
        match frame {
            Message::Text(text) => {
                let seq: usize = match text.as_str().strip_prefix('m').and_then(|n| n.parse().ok())
                {
                    Some(seq) => seq,
                    None => continue,
                };
                let due = schedule
                    .get()
                    .expect("la agenda se publica antes de difundir")
                    .get(seq)
                    .copied()
                    .unwrap_or_else(|| panic!("V17: el mensaje {seq} no se registró"));
                latencies.push(Instant::now().saturating_duration_since(due).as_micros() as u32);
                received += 1;
            }
            Message::Close(frame) => {
                lagged = frame
                    .map(|frame| u16::from(frame.code) == 1013)
                    .unwrap_or(false);
                break;
            }
            _ => {}
        }
    }

    let _ = ws.send(Message::Close(None)).await;
    Outcome {
        received,
        lagged,
        latencies,
    }
}

fn percentile(sorted: &[u32], q: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * q) as usize]
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "medición V17: 1000 conexiones reales a 100 msg/s"]
async fn v17_el_hub_sostiene_mil_conexiones_a_cien_por_segundo() {
    let hub = Hub::new();
    let (addr, shutdown, server) = start(&hub);

    let permits = Arc::new(Semaphore::new(CONNECT_CONCURRENCY));
    // La agenda la publica `main` cuando los 1000 están listos; cada cliente la
    // lee al recibir y compara contra el tick en que debió salir el mensaje.
    let schedule: Arc<OnceLock<Vec<Instant>>> = Arc::new(OnceLock::new());
    let (ready_tx, mut ready_rx) = mpsc::channel::<()>(CLIENTS);
    let mut tasks = Vec::with_capacity(CLIENTS);
    for _ in 0..CLIENTS {
        let permits = Arc::clone(&permits);
        let schedule = Arc::clone(&schedule);
        let ready = ready_tx.clone();
        tasks.push(tokio::spawn(async move {
            run_client(addr, permits, schedule, ready).await
        }));
    }
    drop(ready_tx);

    let deadline = Instant::now() + READY_TIMEOUT;
    let mut ready = 0;
    while ready < CLIENTS {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, ready_rx.recv()).await {
            Ok(Some(())) => ready += 1,
            Ok(None) => panic!("V17: sólo {ready}/{CLIENTS} clientes se saludaron"),
            Err(_) => panic!("V17: timeout con {ready}/{CLIENTS} clientes listos"),
        }
    }
    assert_eq!(
        hub.members(TOPIC).len(),
        CLIENTS,
        "V17: la presencia debe ver las 1000 suscripciones"
    );

    // 100 msg/s durante 500 mensajes: 5 segundos de difusión sostenida.
    // `Burst` recupera los ticks perdidos: si el runtime se satura, la tasa
    // media sigue siendo la exigida y la latencia lo delata.
    let base = Instant::now();
    schedule
        .set((0..MESSAGES).map(|seq| base + TICK * seq as u32).collect())
        .expect("la agenda se publica una sola vez");
    let mut interval = tokio::time::interval_at(tokio::time::Instant::from_std(base), TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
    let mut min_receivers = CLIENTS;
    for seq in 0..MESSAGES {
        interval.tick().await;
        let receivers = hub.broadcast(TOPIC, format!("m{seq}")).await;
        min_receivers = min_receivers.min(receivers);
    }
    let elapsed = base.elapsed();

    let mut outcomes = Vec::with_capacity(CLIENTS);
    for task in tasks {
        outcomes.push(
            tokio::time::timeout(CLIENT_TIMEOUT, task)
                .await
                .expect("V17: un cliente no terminó en 60s")
                .expect("V17: una tarea de cliente entró en pánico"),
        );
    }

    let expected = CLIENTS * MESSAGES;
    let total: usize = outcomes.iter().map(|outcome| outcome.received).sum();
    let lagged: usize = outcomes.iter().filter(|outcome| outcome.lagged).count();
    let short: usize = outcomes
        .iter()
        .filter(|outcome| outcome.received < MESSAGES)
        .count();
    let mut latencies: Vec<u32> = outcomes
        .into_iter()
        .flat_map(|outcome| outcome.latencies)
        .collect();
    latencies.sort_unstable();

    let rate = MESSAGES as f64 / elapsed.as_secs_f64();
    println!(
        "V17: {CLIENTS} conexiones, {MESSAGES} mensajes, {rate:.1} msg/s reales, \
         anillo={} cola={}",
        rdd_socket::DEFAULT_RING,
        rdd_core::http::routes::DEFAULT_SOCKET_OUTBOUND_QUEUE
    );
    println!(
        "V17: recepciones={total}/{expected} ({:.2} %), clientes incompletos={short}, \
         cierres 1013={lagged}, receptores mínimos por difusión={min_receivers}",
        100.0 * total as f64 / expected as f64
    );
    println!(
        "V17: latencia p50={}us p99={}us max={}us",
        percentile(&latencies, 0.50),
        percentile(&latencies, 0.99),
        latencies.last().copied().unwrap_or(0)
    );

    assert_eq!(
        min_receivers, CLIENTS,
        "V17: cada difusión debe alcanzar las 1000 suscripciones"
    );
    assert_eq!(lagged, 0, "V17: ningún cliente debe caer por atraso");
    assert_eq!(
        total, expected,
        "V17: el anillo y la cola deben entregar el 100 % a 100 msg/s"
    );
    println!(
        "V17: VEREDICTO — se mantienen DEFAULT_RING={} y socket_outbound_queue={}",
        rdd_socket::DEFAULT_RING,
        rdd_core::http::routes::DEFAULT_SOCKET_OUTBOUND_QUEUE
    );

    shutdown.trigger();
    tokio::time::timeout(Duration::from_secs(30), server)
        .await
        .expect("V17: la plataforma no cerró")
        .expect("V17: la tarea del servidor entró en pánico");
}
