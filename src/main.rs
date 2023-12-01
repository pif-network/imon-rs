use std::{iter::successors, time::Duration};

use axum::{
    async_trait,
    body::Body,
    extract::{rejection::JsonRejection, FromRequest, Request as AxumExtractRequest, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::NaiveDateTime;
use redis::{Commands, FromRedisValue, JsonCommands};
use serde::{Deserialize, Serialize};
use shuttle_runtime::{CustomError, Error};
use std::net::SocketAddr;
use strum_macros::Display;
use tower_http::{classify::ServerErrorsFailureClass, trace::TraceLayer};
use tracing::{error, info, Span};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum TaskState {
    Begin,
    Break,
    Back,
    End,
    Idle,
}

#[derive(Debug, Display)]
enum UserRecordRedisJsonPath {
    #[strum(serialize = "$")]
    Root,
    #[strum(serialize = "$.task_history")]
    TaskHistory,
    #[strum(serialize = "$.current_task")]
    CurrentTask,
}

#[derive(Debug, Display)]
enum OperatingRedisKey {
    #[strum(serialize = "current_id")]
    CurrentId,
}

#[derive(Serialize, Deserialize, Debug)]
struct Task {
    name: String,
    state: TaskState,
    begin_time: NaiveDateTime,
    end_time: NaiveDateTime,
    duration: i64,
}

impl Default for Task {
    fn default() -> Self {
        Task {
            name: String::new(),
            state: TaskState::Begin,
            begin_time: chrono::NaiveDateTime::from_timestamp_opt(0, 0).unwrap(),
            end_time: chrono::NaiveDateTime::from_timestamp_opt(0, 0).unwrap(),
            duration: 0,
        }
    }
}

impl Task {
    fn placeholder(name: &str, state: TaskState) -> Self {
        Task {
            name: name.to_string(),
            state,
            ..Task::default()
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct UserRecord {
    id: i32,
    user_name: String,
    task_history: Vec<Task>,
    current_task: Task,
}

#[derive(Serialize, Deserialize, Debug)]
struct StoreTaskPayload {
    user_name: String,
    task: Task,
}

#[derive(Serialize, Deserialize, Debug)]
struct RegisterRecordPayload {
    user_name: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ResetUserDataPayload {
    key: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct GetTaskLogPayload {
    key: String,
}

impl FromRedisValue for UserRecord {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<UserRecord> {
        match *v {
            redis::Value::Data(ref bytes) => {
                let user_data: UserRecord = serde_json::from_slice(bytes)?;
                Ok(user_data)
            }
            _ => Err((redis::ErrorKind::TypeError, "Invalid type").into()),
        }
    }
}

fn perform_store_task(
    payload: StoreTaskPayload,
    redis_client: redis::Client,
) -> Result<(), redis::RedisError> {
    let mut con = redis_client.get_connection()?;
    match con.json_get::<&std::string::String, &str, Option<String>>(
        &payload.user_name,
        UserRecordRedisJsonPath::Root.to_string().as_str(),
    ) {
        Ok(data_str) => match data_str {
            Some(data_str) => {
                let mut user_data: Vec<UserRecord> = serde_json::from_str(&data_str).unwrap();
                println!("user_data: {:?}", user_data);

                // Remove the latest task from the history
                // to append the updated version later.
                if user_data[0].current_task.state == TaskState::Begin
                    || user_data[0].current_task.state == TaskState::Break
                    || user_data[0].current_task.state == TaskState::Back
                {
                    user_data[0].task_history.pop();
                };

                con.json_set(
                    &payload.user_name,
                    UserRecordRedisJsonPath::TaskHistory.to_string().as_str(),
                    &serde_json::json!(&user_data.into_iter().next().unwrap().task_history),
                )?;

                println!("appending");
                con.json_arr_append(
                    &payload.user_name,
                    UserRecordRedisJsonPath::TaskHistory.to_string().as_str(),
                    &serde_json::json!(&payload.task),
                )?;

                println!("setting current task");
                con.json_set(
                    &payload.user_name,
                    UserRecordRedisJsonPath::CurrentTask.to_string().as_str(),
                    &serde_json::json!(&payload.task),
                )?;

                Ok(())
            }
            None => Err(redis::RedisError::from((
                redis::ErrorKind::ResponseError,
                // Redis gives nil -> no key -> no user.
                "User not found.",
            ))),
        },
        Err(err) => {
            println!("err: {:?}", err);
            return Err(err);
        }
    }
}

fn perform_reset_task(
    payload: ResetUserDataPayload,
    redis_client: redis::Client,
) -> Result<UserRecord, redis::RedisError> {
    let mut con = redis_client.get_connection()?;
    match con.json_get::<&std::string::String, &str, Option<String>>(
        &payload.key,
        UserRecordRedisJsonPath::Root.to_string().as_str(),
    ) {
        Ok(data_str) => match data_str {
            Some(_data_str) => {
                let user_data = UserRecord {
                    id: payload.key.split(":").collect::<Vec<&str>>()[1]
                        .parse::<i32>()
                        .unwrap(),
                    user_name: payload.key.split(":").collect::<Vec<&str>>()[0].to_string(),
                    task_history: vec![],
                    current_task: Task::placeholder("reset", TaskState::Idle),
                };
                con.json_set(
                    &payload.key,
                    UserRecordRedisJsonPath::Root.to_string().as_str(),
                    &serde_json::json!(user_data),
                )?;

                Ok(user_data)
            }
            None => Err(redis::RedisError::from((
                redis::ErrorKind::ResponseError,
                // Redis gives nil -> no key -> no user.
                "User not found.",
            ))),
        },
        Err(err) => {
            println!("err: {:?}", err);
            return Err(err);
        }
    }
}

fn generate_key(user_name: &str, id: i32) -> String {
    let id_length = successors(Some(id), |&n| (n >= 10).then(|| n / 10)).count();
    let filler_length = 4 - id_length;
    format!("{}:{}{}", user_name, "0".repeat(filler_length), id)
}

fn perform_register_record(
    payload: RegisterRecordPayload,
    redis_client: redis::Client,
) -> Result<String, redis::RedisError> {
    let mut con = redis_client.get_connection()?;

    let new_id;

    match con.get::<&str, i32>(OperatingRedisKey::CurrentId.to_string().as_str()) {
        Ok(current_id) => {
            new_id = current_id + 1;
            con.set("current_id", new_id)?;
        }
        Err(err) => {
            new_id = 0;
            con.set("current_id", 0)?;

            println!("err: {:?}", err);
        }
    }

    let user_key = generate_key(&payload.user_name, new_id);

    let user_data = UserRecord {
        id: new_id,
        user_name: payload.user_name,
        task_history: vec![],
        current_task: Task::placeholder("initialised", TaskState::Idle),
    };
    con.json_set(
        &user_key,
        UserRecordRedisJsonPath::Root.to_string().as_str(),
        &serde_json::json!(user_data),
    )?;

    println!("new user: {:?}", user_data);

    Ok(user_key)
}

fn perform_get_user_task_log(
    payload: GetTaskLogPayload,
    redis_client: redis::Client,
) -> Result<UserRecord, redis::RedisError> {
    let mut con = redis_client.get_connection()?;

    match con.json_get::<&std::string::String, &str, Option<String>>(
        &payload.key,
        UserRecordRedisJsonPath::Root.to_string().as_str(),
    ) {
        Ok(data_str) => match data_str {
            Some(data_str) => {
                let user_data: Vec<UserRecord> =
                    serde_json::from_str(&data_str).expect("Parsing `user_data` should not fail.");

                Ok(user_data.into_iter().next().unwrap())
            }
            None => Err(redis::RedisError::from((
                redis::ErrorKind::ResponseError,
                // Redis gives nil -> no key -> no user.
                "User not found.",
            ))),
        },
        Err(err) => {
            println!("err: {:?}", err);
            return Err(err);
        }
    }
}

fn perform_get_all_records(
    redis_client: redis::Client,
) -> Result<Vec<UserRecord>, redis::RedisError> {
    let mut con = redis_client.get_connection()?;

    // FIXME: Multiple borrows of `con` are not allowed.
    match redis_client.get_connection()?.scan_match("*:????") {
        Ok(keys) => {
            let mut user_records: Vec<UserRecord> = vec![];

            for key in keys {
                match con.json_get::<&std::string::String, &str, Option<String>>(
                    &key,
                    UserRecordRedisJsonPath::Root.to_string().as_str(),
                ) {
                    Ok(data_str) => match data_str {
                        Some(data_str) => {
                            let user_data: Vec<UserRecord> = serde_json::from_str(&data_str)
                                .expect("Parsing `user_data` should not fail.");
                            println!("user_data: {:?}", user_data);
                            user_records.push(user_data.into_iter().next().unwrap());
                        }
                        None => {
                            println!("User not found.");
                        }
                    },
                    Err(err) => {
                        println!("err: {:?}", err);
                        return Err(err);
                    }
                }
            }

            Ok(user_records)
        }
        Err(err) => {
            println!("err: {:?}", err);
            return Err(err);
        }
    }
}

fn construct_redis_error_response(err: redis::RedisError) -> serde_json::Value {
    match err.kind() {
        redis::ErrorKind::ResponseError => serde_json::json!({
            "status": "error",
            // FIXME: Most of the time, this error means that the user has not
            // registered yet, but it is still not the best way to handle.
            "message": "Invalid credentials",
        }),
        _ => serde_json::json!({
            "status": "error",
            "message": err.to_string(),
        }),
    }
}

fn construct_json_error_response(err: &JsonRejection) -> serde_json::Value {
    serde_json::json!({
        "status": "error",
        "message": err.to_string(),
    })
}

#[derive(Debug)]
pub struct ValidatedJson<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    axum::Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, axum::Json<serde_json::Value>);

    async fn from_request(req: AxumExtractRequest, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(json) => Ok(Self(json.0)),
            Err(rejection) => {
                let payload = construct_json_error_response(&rejection);
                error!("rejection: {:?}", rejection);
                Err((rejection.status(), axum::Json(payload)))
            }
        }
    }
}

async fn store_task(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<StoreTaskPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match perform_store_task(payload, app_state.redis_client) {
        Ok(_) => Ok(Json(serde_json::json!({
            "status": "ok",
        }))),
        Err(err) => {
            let error_response = construct_redis_error_response(err);
            Err((StatusCode::BAD_REQUEST, Json(error_response)))
        }
    }
}

async fn reset_task(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<ResetUserDataPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match perform_reset_task(payload, app_state.redis_client) {
        Ok(user_data) => Ok(Json(serde_json::json!({
            "status": "ok",
            "data": {
                "user_data": user_data,
            }
        }))),
        Err(err) => {
            let error_response = construct_redis_error_response(err);
            Err((StatusCode::BAD_REQUEST, Json(error_response)))
        }
    }
}

async fn register_record(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<RegisterRecordPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match perform_register_record(payload, app_state.redis_client) {
        Ok(user_key) => Ok(Json(serde_json::json!({
            "status": "ok",
            "data": {
                "user_key": user_key,
            }
        }))),
        Err(err) => {
            let error_response = construct_redis_error_response(err);
            Err((StatusCode::BAD_REQUEST, Json(error_response)))
        }
    }
}

async fn get_all_records(
    State(app_state): State<AppState>,
    // ValidatedJson(payload): ValidatedJson<RegisterRecordPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match perform_get_all_records(app_state.redis_client) {
        Ok(user_records) => Ok(Json(serde_json::json!({
            "status": "ok",
            "data": {
                "user_records": user_records,
            }
        }))),
        Err(err) => {
            let error_response = construct_redis_error_response(err);
            Err((StatusCode::BAD_REQUEST, Json(error_response)))
        }
    }
}

async fn get_task_log(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<GetTaskLogPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match perform_get_user_task_log(payload, app_state.redis_client) {
        Ok(task_log) => Ok(Json(serde_json::json!({
            "status": "ok",
            "data": {
                "task_log": task_log,
            }
        }))),
        Err(err) => {
            let error_response = construct_redis_error_response(err);
            Err((StatusCode::BAD_REQUEST, Json(error_response)))
        }
    }
}

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
struct AppState {
    redis_client: redis::Client,
}

#[shuttle_runtime::main]
// async fn axum() -> shuttle_axum::ShuttleAxum {
async fn axum() -> PShuttleAxum {
    let client = redis::Client::open(
        "rediss://default:c133fb0ebf6341f4a7a58c9a648b353e@apn1-sweet-haddock-33446.upstash.io:33446",
    ).expect("Redis client should be created successfully."); // FIXME: Handle the error

    let app_state = AppState {
        redis_client: client,
    };

    let router = Router::new()
        .route("/v1/store", post(store_task))
        .route("/v1/reset", post(reset_task))
        .route("/v1/record/new", post(register_record))
        .route("/v1/record/all", get(get_all_records))
        .route("/v1/task-log", post(get_task_log))
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
