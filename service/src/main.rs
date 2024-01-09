use std::time::Duration;

use axum::{
    body::Body,
    http::Request,
    response::Response,
    routing::{get, post},
    Router,
};
use bb8_redis::{bb8::Pool, redis::JsonAsyncCommands, RedisConnectionManager};
use libs::{OperatingInfoRedisJsonPath, OperatingRedisKey};
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

async fn check_or_init_operating_record(redis_pool: Pool<RedisConnectionManager>) {
    let mut con = redis_pool.get().await.unwrap();

    match con
        .json_get::<&str, &str, Option<String>>(
            OperatingRedisKey::OperatingInfo.to_string().as_str(),
            OperatingInfoRedisJsonPath::Root.to_string().as_str(),
        )
        .await
        .unwrap()
    {
        Some(_) => {
            tracing::info!("Check: `operating_info` exists.");
        }
        None => {
            tracing::info!("Check: `operating_info` doesn't exist. Creating");
            let operating_info = libs::OperatingInfo {
                latest_record_id: 0,
                latest_sudo_record_id: 0,
            };
            let _: () = con
                .json_set(
                    OperatingRedisKey::OperatingInfo.to_string().as_str(),
                    OperatingInfoRedisJsonPath::Root.to_string().as_str(),
                    &serde_json::json!(operating_info),
                )
                .await
                .unwrap();
        }
    };
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

    check_or_init_operating_record(pool.clone()).await;

    let app_state = AppState { redis_pool: pool };

    let router = Router::new()
        .route("/v1/rpc/sudo", post(handlers::sudo_user_rpc))
        .route("/v1/record/new", post(handlers::register_record))
        .route("/v1/record", post(handlers::get_user_record))
        .route("/v1/record/all", get(handlers::get_all_user_records))
        .route("/v1/task/new", post(handlers::create_task))
        .route("/v1/task/reset", post(handlers::reset_task))
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
