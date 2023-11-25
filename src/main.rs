use std::{iter::successors, time::Duration};

use axum::{
    body::Body,
    extract,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chrono::NaiveDateTime;
use redis::{Commands, FromRedisValue, JsonCommands};
use serde::{Deserialize, Serialize};
use strum_macros::Display;
use tower_http::{classify::ServerErrorsFailureClass, trace::TraceLayer};
use tracing::{info, Span};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum TaskState {
    Begin,
    Break,
    Back,
    End,
    Idle,
}

#[derive(Debug, Display)]
enum RedisKey {
    #[strum(serialize = "$")]
    Root,
    #[strum(serialize = "$.task_history")]
    TaskHistory,
    #[strum(serialize = "$.current_task")]
    CurrentTask,
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
struct UserData {
    id: i32,
    task_history: Vec<Task>,
    current_task: Task,
}

#[derive(Serialize, Deserialize, Debug)]
struct StoreTaskPayload {
    user_name: String,
    task: Task,
}

#[derive(Serialize, Deserialize, Debug)]
struct UpdateCredentialsPayload {
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

impl FromRedisValue for UserData {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<UserData> {
        match *v {
            redis::Value::Data(ref bytes) => {
                let user_data: UserData = serde_json::from_slice(bytes)?;
                Ok(user_data)
            }
            _ => Err((redis::ErrorKind::TypeError, "Invalid type").into()),
        }
    }
}

fn perform_store_task(payload: StoreTaskPayload) -> Result<(), redis::RedisError> {
    let client = redis::Client::open(
        "rediss://default:c133fb0ebf6341f4a7a58c9a648b353e@apn1-sweet-haddock-33446.upstash.io:33446",
        // "redis://default:ErYxrixFKO55MaU9O5xDmPs1SLsz78Ji@redis-15313.c54.ap-northeast-1-2.ec2.cloud.redislabs.com:15313",
    )?;
    let mut con = client.get_connection()?;
    match con.json_get::<&std::string::String, &str, Option<String>>(
        &payload.user_name,
        RedisKey::Root.to_string().as_str(),
    ) {
        Ok(data_str) => match data_str {
            Some(data_str) => {
                let mut user_data: Vec<UserData> = serde_json::from_str(&data_str).unwrap();
                println!("user_data: {:?}", user_data);

                // Remove the latest task from the history
                // to append the updated version later.
                user_data[0].task_history.pop();
                con.json_set(
                    &payload.user_name,
                    RedisKey::TaskHistory.to_string().as_str(),
                    &serde_json::json!(&user_data.into_iter().next().unwrap().task_history),
                )?;

                println!("appending");
                con.json_arr_append(
                    &payload.user_name,
                    RedisKey::TaskHistory.to_string().as_str(),
                    &serde_json::json!(&payload.task),
                )?;

                println!("setting current task");
                con.json_set(
                    &payload.user_name,
                    RedisKey::CurrentTask.to_string().as_str(),
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

fn perform_reset_task(payload: ResetUserDataPayload) -> Result<UserData, redis::RedisError> {
    let client = redis::Client::open(
        "rediss://default:c133fb0ebf6341f4a7a58c9a648b353e@apn1-sweet-haddock-33446.upstash.io:33446",
        // "redis://default:ErYxrixFKO55MaU9O5xDmPs1SLsz78Ji@redis-15313.c54.ap-northeast-1-2.ec2.cloud.redislabs.com:15313",
    )?;
    let mut con = client.get_connection()?;

    let user_data = UserData {
        id: payload.key.split(":").collect::<Vec<&str>>()[1]
            .parse::<i32>()
            .unwrap(),
        task_history: vec![],
        current_task: Task::placeholder("reset", TaskState::Idle),
    };
    con.json_set(
        &payload.key,
        RedisKey::Root.to_string().as_str(),
        &serde_json::json!(user_data),
    )?;

    Ok(user_data)
}

fn generate_key(user_name: &str, id: i32) -> String {
    let id_length = successors(Some(id), |&n| (n >= 10).then(|| n / 10)).count();
    let filler_length = 4 - id_length;
    format!("{}:{}{}", user_name, "0".repeat(filler_length), id)
}

fn perform_update_credentials(
    payload: UpdateCredentialsPayload,
) -> Result<String, redis::RedisError> {
    let client = redis::Client::open(
        "rediss://default:c133fb0ebf6341f4a7a58c9a648b353e@apn1-sweet-haddock-33446.upstash.io:33446",
        // "redis://default:ErYxrixFKO55MaU9O5xDmPs1SLsz78Ji@redis-15313.c54.ap-northeast-1-2.ec2.cloud.redislabs.com:15313",
    )?;
    let mut con = client.get_connection()?;

    let new_id;

    match con.get::<&str, i32>("current_id") {
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

    let user_data = UserData {
        id: new_id,
        task_history: vec![],
        current_task: Task::placeholder("initialised", TaskState::Idle),
    };
    con.json_set(
        &user_key,
        RedisKey::Root.to_string().as_str(),
        &serde_json::json!(user_data),
    )?;

    println!("new user: {:?}", user_data);

    Ok(user_key)
}

fn perform_get_user_task_log(payload: GetTaskLogPayload) -> Result<UserData, redis::RedisError> {
    let client = redis::Client::open(
        "rediss://default:c133fb0ebf6341f4a7a58c9a648b353e@apn1-sweet-haddock-33446.upstash.io:33446",
        // "redis://default:ErYxrixFKO55MaU9O5xDmPs1SLsz78Ji@redis-15313.c54.ap-northeast-1-2.ec2.cloud.redislabs.com:15313",
    )?;
    let mut con = client.get_connection()?;

    match con.json_get::<&std::string::String, &str, Option<String>>(
        &payload.key,
        RedisKey::Root.to_string().as_str(),
    ) {
        Ok(data_str) => match data_str {
            Some(data_str) => {
                let user_data: Vec<UserData> = serde_json::from_str(&data_str).unwrap();

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

fn construct_error_response(err: redis::RedisError) -> serde_json::Value {
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

async fn store_task(
    extract::Json(payload): extract::Json<StoreTaskPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match perform_store_task(payload) {
        Ok(_) => Ok(Json(serde_json::json!({
            "status": "ok",
        }))),
        Err(err) => {
            let error_response = construct_error_response(err);
            Err((StatusCode::BAD_REQUEST, Json(error_response)))
        }
    }
}

async fn reset_task(
    extract::Json(payload): extract::Json<ResetUserDataPayload>,
) -> Json<serde_json::Value> {
    let user_data = perform_reset_task(payload).unwrap();

    Json(serde_json::json!({
        "status": "ok",
        "data": user_data,
    }))
}

async fn update_credentials(
    extract::Json(payload): extract::Json<UpdateCredentialsPayload>,
) -> Json<serde_json::Value> {
    println!("payload: {:?}", payload);
    let user_key = perform_update_credentials(payload).unwrap();
    Json(serde_json::json!({
        "status": "ok",
        "data": {
            "user_key": user_key,
        }
    }))
}

async fn get_task_log(
    extract::Json(payload): extract::Json<GetTaskLogPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match perform_get_user_task_log(payload) {
        Ok(task_log) => Ok(Json(serde_json::json!({
            "status": "ok",
            "data": {
                "task_log": task_log,
            }
        }))),
        Err(err) => {
            let error_response = construct_error_response(err);
            Err((StatusCode::BAD_REQUEST, Json(error_response)))
        }
    }
}

#[shuttle_runtime::main]
async fn axum() -> shuttle_axum::ShuttleAxum {
    let router = Router::new()
        .route("/v1/store", post(store_task))
        .route("/v1/reset", post(reset_task))
        .route("/v1/credentials", post(update_credentials))
        .route("/v1/task-log", post(get_task_log))
        .layer(
            TraceLayer::new_for_http()
                .on_request(|request: &Request<Body>, _span: &Span| {
                    info!("{:?} {:?}", request.method(), request.uri());
                })
                .on_response(|_response: &Response, _latency: Duration, _span: &Span| {
                    // ...
                })
                .on_failure(
                    |_error: ServerErrorsFailureClass, _latency: Duration, _span: &Span| {
                        // ...
                    },
                ),
        );

    Ok(router.into())
}
