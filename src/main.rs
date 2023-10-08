use std::iter::successors;

use axum::{extract, routing::post, Json, Router};
use chrono::NaiveDateTime;
use redis::{Commands, FromRedisValue, JsonCommands};
use serde::{Deserialize, Serialize};
use strum_macros::Display;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum TaskState {
    Begin,
    Break,
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
    duration: String,
}

impl Default for Task {
    fn default() -> Self {
        Task {
            name: String::new(),
            state: TaskState::Begin,
            begin_time: chrono::NaiveDateTime::from_timestamp_opt(0, 0).unwrap(),
            end_time: chrono::NaiveDateTime::from_timestamp_opt(0, 0).unwrap(),
            duration: String::new(),
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
    match con.json_get::<&std::string::String, &str, Vec<UserData>>(
        &payload.user_name,
        RedisKey::Root.to_string().as_str(),
    ) {
        Ok(data) => {
            println!("user_data: {:?}", data);
        }
        Err(err) => {
            println!("err: {:?}", err);
        }
    }

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

fn perform_reset_task(payload: ResetUserDataPayload) -> Result<UserData, redis::RedisError> {
    let client = redis::Client::open(
        "rediss://default:c133fb0ebf6341f4a7a58c9a648b353e@apn1-sweet-haddock-33446.upstash.io:33446",
        // "redis://default:ErYxrixFKO55MaU9O5xDmPs1SLsz78Ji@redis-15313.c54.ap-northeast-1-2.ec2.cloud.redislabs.com:15313",
    )?;
    let mut con = client.get_connection()?;

    let user_data = UserData {
        id: payload.key.parse::<i32>().unwrap(),
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

fn perform_update_credentials(payload: UpdateCredentialsPayload) -> Result<(), redis::RedisError> {
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

    let user_data = UserData {
        id: new_id,
        task_history: vec![],
        current_task: Task::placeholder("initialised", TaskState::Idle),
    };
    con.json_set(
        generate_key(&payload.user_name, new_id),
        RedisKey::Root.to_string().as_str(),
        &serde_json::json!(user_data),
    )?;

    println!("new user: {:?}", user_data);

    Ok(())
}

async fn store_task(
    extract::Json(payload): extract::Json<StoreTaskPayload>,
) -> Json<serde_json::Value> {
    perform_store_task(payload).unwrap();

    Json(serde_json::json!({
        "status": "ok",
    }))
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
    perform_update_credentials(payload).unwrap();
    Json(serde_json::json!({
        "status": "ok",
    }))
}

#[shuttle_runtime::main]
async fn axum() -> shuttle_axum::ShuttleAxum {
    let router = Router::new()
        .route("/v1/store", post(store_task))
        .route("/v1/reset", post(reset_task))
        .route("/v1/credentials", post(update_credentials));

    Ok(router.into())
}
