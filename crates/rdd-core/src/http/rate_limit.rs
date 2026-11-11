//! Rate limiting por ventana fija para cualquier plataforma HTTP.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::{Error, Problem};
use crate::http::response::problem_response;
use crate::http::types::{Middleware, NextArc, Request, Response, StatusCode};

const DEFAULT_MAX_KEYS: usize = 10_000;

/// Límite de peticiones en memoria para cada clave elegida por la aplicación.
///
/// El middleware no presupone cómo identificar al cliente: el transporte del
/// núcleo no expone una dirección remota y confiar en `X-Forwarded-For` sin un
/// proxy confiable permite falsificar la identidad. Usa una identidad ya
/// autenticada o un API key validado; una cabecera arbitraria permite eludir el
/// límite cambiando su valor.
/// La autenticación debe validar las credenciales e insertar la identidad en
/// las extensiones de la petición antes de que se ejecute este middleware.
///
/// Este límite es local al proceso. Para varias réplicas, usa un proxy o un
/// almacenamiento compartido; un limitador en memoria no coordina procesos.
///
/// ```
/// use std::time::Duration;
///
/// use rdd_core::prelude::*;
///
/// struct AuthenticatedUser {
///     id: u64,
/// }
///
/// let rate_limit = RateLimit::new(60, Duration::from_secs(60), |req| {
///     req.extensions()
///         .get::<AuthenticatedUser>()
///         .map(|user| format!("user:{}", user.id))
///         .unwrap_or_else(|| "anonymous".to_owned())
/// })
/// .expect("la configuración del rate limit es válida");
/// let app = App::new().middleware(rate_limit);
/// # let _ = app;
/// ```
pub struct RateLimit<F> {
    limit: u32,
    window: Duration,
    key: F,
    max_keys: usize,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl<F> RateLimit<F>
where
    F: Fn(&Request) -> String + Send + Sync + 'static,
{
    /// Crea un límite por clave y ventana. El contador reinicia al terminar cada
    /// ventana desde la primera petición de esa clave.
    pub fn new(limit: u32, window: Duration, key: F) -> Result<Self, RateLimitConfigError> {
        if limit == 0 {
            return Err(RateLimitConfigError::ZeroLimit);
        }
        if window.is_zero() {
            return Err(RateLimitConfigError::ZeroWindow);
        }

        Ok(Self {
            limit,
            window,
            key,
            max_keys: DEFAULT_MAX_KEYS,
            buckets: Mutex::new(HashMap::new()),
        })
    }

    /// Máximo de claves de cliente conservadas simultáneamente (predeterminado:
    /// 10 000). Si se alcanza y no hay ventanas expiradas que liberar, el
    /// middleware responde 503 en vez de crecer sin límite.
    pub fn with_max_keys(mut self, max_keys: usize) -> Result<Self, RateLimitConfigError> {
        if max_keys == 0 {
            return Err(RateLimitConfigError::ZeroMaxKeys);
        }
        self.max_keys = max_keys;
        Ok(self)
    }

    fn admit(&self, key: String) -> Result<Decision, AdmissionError> {
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| AdmissionError::StatePoisoned)?;
        let now = Instant::now();

        if !buckets.contains_key(&key) && buckets.len() >= self.max_keys {
            buckets.retain(|_, bucket| now.duration_since(bucket.last_seen) < self.window);
            if buckets.len() >= self.max_keys {
                return Err(AdmissionError::Capacity);
            }
        }

        let bucket = buckets.entry(key).or_insert(Bucket {
            started_at: now,
            last_seen: now,
            count: 0,
        });
        if now.duration_since(bucket.started_at) >= self.window {
            bucket.started_at = now;
            bucket.count = 0;
        }
        bucket.last_seen = now;

        let reset_after = self
            .window
            .saturating_sub(now.duration_since(bucket.started_at));
        if bucket.count >= self.limit {
            return Ok(Decision {
                allowed: false,
                remaining: 0,
                reset_after,
            });
        }

        bucket.count += 1;
        Ok(Decision {
            allowed: true,
            remaining: self.limit - bucket.count,
            reset_after,
        })
    }
}

impl<F> Middleware for RateLimit<F>
where
    F: Fn(&Request) -> String + Send + Sync + 'static,
{
    fn handle<'a>(
        &'a self,
        req: Request,
        next: NextArc,
    ) -> Pin<Box<dyn Future<Output = crate::Result<Response>> + Send + 'a>> {
        Box::pin(async move {
            let key = (self.key)(&req);
            let decision = match self.admit(key) {
                Ok(decision) => decision,
                Err(AdmissionError::Capacity) => {
                    tracing::warn!(
                        max_keys = self.max_keys,
                        "se alcanzó la capacidad de claves del rate limiter"
                    );
                    return rate_limit_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Service Unavailable",
                        "el limitador de peticiones alcanzó su capacidad configurada",
                        &req,
                        self.limit,
                        0,
                        self.window,
                    );
                }
                Err(AdmissionError::StatePoisoned) => {
                    return Err(Error::internal(anyhow::anyhow!(
                        "el estado del rate limiter está envenenado"
                    )));
                }
            };

            if !decision.allowed {
                return rate_limit_error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Too Many Requests",
                    "se superó el límite de peticiones para esta ventana",
                    &req,
                    self.limit,
                    decision.remaining,
                    decision.reset_after,
                );
            }

            let mut response = next(req).await?;
            set_rate_headers(
                &mut response,
                self.limit,
                decision.remaining,
                decision.reset_after,
            )?;
            Ok(response)
        })
    }
}

/// Error de configuración de [`RateLimit`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RateLimitConfigError {
    /// El máximo debe ser mayor que cero.
    #[error("el límite de peticiones debe ser mayor que cero")]
    ZeroLimit,
    /// La duración de la ventana debe ser mayor que cero.
    #[error("la ventana del rate limiter debe ser mayor que cero")]
    ZeroWindow,
    /// La capacidad del estado debe ser mayor que cero.
    #[error("el máximo de claves del rate limiter debe ser mayor que cero")]
    ZeroMaxKeys,
}

struct Bucket {
    started_at: Instant,
    last_seen: Instant,
    count: u32,
}

struct Decision {
    allowed: bool,
    remaining: u32,
    reset_after: Duration,
}

enum AdmissionError {
    Capacity,
    StatePoisoned,
}

fn rate_limit_error(
    status: StatusCode,
    title: &str,
    detail: &str,
    req: &Request,
    limit: u32,
    remaining: u32,
    reset_after: Duration,
) -> crate::Result<Response> {
    let problem = Problem {
        r#type: "about:blank".to_owned(),
        title: title.to_owned(),
        status: status.as_u16(),
        detail: detail.to_owned(),
        instance: req.uri().path().to_owned(),
    };
    let mut response = problem_response(status, &problem, None);
    set_rate_headers(&mut response, limit, remaining, reset_after)?;
    if status == StatusCode::TOO_MANY_REQUESTS {
        response.headers_mut().insert(
            http::header::RETRY_AFTER,
            header_value(seconds_ceil(reset_after))
                .map_err(|err| Error::internal(anyhow::Error::new(err)))?,
        );
    }
    Ok(response)
}

fn set_rate_headers(
    response: &mut Response,
    limit: u32,
    remaining: u32,
    reset_after: Duration,
) -> crate::Result<()> {
    response.headers_mut().insert(
        "ratelimit-limit",
        header_value(limit).map_err(|err| Error::internal(anyhow::Error::new(err)))?,
    );
    response.headers_mut().insert(
        "ratelimit-remaining",
        header_value(remaining).map_err(|err| Error::internal(anyhow::Error::new(err)))?,
    );
    response.headers_mut().insert(
        "ratelimit-reset",
        header_value(seconds_ceil(reset_after))
            .map_err(|err| Error::internal(anyhow::Error::new(err)))?,
    );
    Ok(())
}

fn header_value(
    value: impl std::fmt::Display,
) -> Result<http::HeaderValue, http::header::InvalidHeaderValue> {
    http::HeaderValue::from_str(&value.to_string())
}

fn seconds_ceil(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() > 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn request(key: &str) -> Request {
        http::Request::builder()
            .uri("/tasks")
            .header("x-client-key", key)
            .body(Bytes::new())
            .expect("la petición de prueba es válida")
    }

    fn next(calls: Arc<AtomicUsize>) -> NextArc {
        Arc::new(move |_req| {
            calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(http::Response::new(Bytes::new())) })
        })
    }

    #[tokio::test]
    async fn limita_por_clave_y_devuelve_problem_details_y_cabeceras() {
        let limiter = RateLimit::new(1, Duration::from_secs(60), |req: &Request| {
            req.headers()["x-client-key"].to_str().unwrap().to_owned()
        })
        .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let next = next(Arc::clone(&calls));

        let first = limiter
            .handle(request("alice"), Arc::clone(&next))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(first.headers()["ratelimit-limit"], "1");
        assert_eq!(first.headers()["ratelimit-remaining"], "0");

        let limited = limiter
            .handle(request("alice"), Arc::clone(&next))
            .await
            .unwrap();
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            limited.headers()["content-type"],
            "application/problem+json"
        );
        assert!(limited.headers().contains_key("retry-after"));
        let problem: serde_json::Value = serde_json::from_slice(limited.body()).unwrap();
        assert_eq!(problem["status"], 429);

        let separate_key = limiter.handle(request("bob"), next).await.unwrap();
        assert_eq!(separate_key.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn reinicia_el_contador_tras_la_ventana() {
        let limiter = RateLimit::new(1, Duration::from_millis(10), |_req: &Request| {
            "shared".to_owned()
        })
        .unwrap();
        let next = next(Arc::new(AtomicUsize::new(0)));

        assert_eq!(
            limiter
                .handle(request("alice"), Arc::clone(&next))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        tokio::time::sleep(Duration::from_millis(15)).await;
        assert_eq!(
            limiter
                .handle(request("alice"), next)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aplica_el_limite_concurrentemente() {
        let limiter = std::sync::Arc::new(
            RateLimit::new(2, Duration::from_secs(60), |_req: &Request| {
                "shared".to_owned()
            })
            .unwrap(),
        );
        let next = next(Arc::new(AtomicUsize::new(0)));
        let mut requests = Vec::new();

        for _ in 0..32 {
            let limiter = Arc::clone(&limiter);
            let next = Arc::clone(&next);
            requests.push(tokio::spawn(async move {
                limiter
                    .handle(request("client"), next)
                    .await
                    .unwrap()
                    .status()
            }));
        }

        let mut allowed = 0;
        for request in requests {
            if request.await.unwrap() == StatusCode::OK {
                allowed += 1;
            }
        }
        assert_eq!(allowed, 2);
    }

    #[tokio::test]
    async fn rechaza_nuevas_claves_si_se_agota_la_capacidad() {
        let limiter = RateLimit::new(1, Duration::from_secs(60), |req: &Request| {
            req.headers()["x-client-key"].to_str().unwrap().to_owned()
        })
        .unwrap()
        .with_max_keys(1)
        .unwrap();
        let next = next(Arc::new(AtomicUsize::new(0)));

        assert_eq!(
            limiter
                .handle(request("alice"), Arc::clone(&next))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            limiter.handle(request("bob"), next).await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn rechaza_configuracion_invalida() {
        assert!(matches!(
            RateLimit::new(0, Duration::from_secs(1), |_req: &Request| String::new()),
            Err(RateLimitConfigError::ZeroLimit)
        ));
        assert!(matches!(
            RateLimit::new(1, Duration::ZERO, |_req: &Request| String::new()),
            Err(RateLimitConfigError::ZeroWindow)
        ));
        assert!(matches!(
            RateLimit::new(1, Duration::from_secs(1), |_req: &Request| String::new())
                .unwrap()
                .with_max_keys(0),
            Err(RateLimitConfigError::ZeroMaxKeys)
        ));
    }
}
