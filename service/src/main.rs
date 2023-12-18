use std::time::Duration;

use axum::{
    body::Body,
    http::Request,
    response::Response,
    routing::{get, post},
    Router,
};
use bb8_redis::{bb8::Pool, RedisConnectionManager};
use shuttle_runtime::{CustomError, Error};
use std::net::SocketAddr;
use tower_http::{classify::ServerErrorsFailureClass, trace::TraceLayer};
use tracing::{error, info, Span};

mod presenter;
use presenter::handlers;

pub struct AxumService(pub axum::Router);

#[shuttle_runtime::async_trait]
impl shuttle_runtime::Service for AxumService {
    async fn bind(mut self, addr: SocketAddr) -> Result<(), Error> {
        let tcp_listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(tcp_listener, self.0.into_make_service())
            .await
            .map_err(CustomError::new)?;

        Ok(())
    }
}

impl From<axum::Router> for AxumService {
    fn from(router: axum::Router) -> Self {
        Self(router)
    }
}

type PShuttleAxum = Result<AxumService, Error>;

#[derive(Clone)]
pub struct AppState {
    // redis_client: redis::Client,
    redis_pool: Pool<RedisConnectionManager>,
}

#[shuttle_runtime::main]
// async fn axum() -> shuttle_axum::ShuttleAxum {
async fn axum() -> PShuttleAxum {
    let redis_manager = RedisConnectionManager::new("rediss://default:c133fb0ebf6341f4a7a58c9a648b353e@apn1-sweet-haddock-33446.upstash.io:33446")
        .expect("Redis connection URL should be valid");
    let pool = bb8_redis::bb8::Pool::builder()
        .min_idle(Some(4))
        .build(redis_manager)
        .await
        .unwrap();

    let app_state = AppState { redis_pool: pool };

    let router = Router::new()
        .route("/v1/store", post(handlers::store_task))
        .route("/v1/reset", post(handlers::reset_task))
        .route("/v1/record/new", post(handlers::register_record))
        .route("/v1/record/all", get(handlers::get_all_records))
        .route("/v1/task-log", post(handlers::get_task_log))
        .route("/v1/task/update", post(handlers::update_task_log))
        .layer(
            TraceLayer::new_for_http()
                .on_request(|request: &Request<Body>, _span: &Span| {
                    info!("{:?} {:?}", request.method(), request.uri());
                })
                .on_response(|response: &Response, _latency: Duration, _span: &Span| {
                    if response.status().is_success() {
                        info!("{:?}", response.status());
                    } else {
                        error!("{:?}", response.status());
                    }
                })
                .on_failure(
                    |_error: ServerErrorsFailureClass, _latency: Duration, _span: &Span| {
                        // ...
                    },
                ),
        )
        .with_state(app_state);

    Ok(router.into())
}
