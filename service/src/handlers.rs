use std::iter::successors;

use axum::{
    async_trait,
    extract::{rejection::JsonRejection, FromRequest, Request as AxumExtractRequest, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use redis::{Commands, JsonCommands};
use serde::{Deserialize, Serialize};

use libs::{
    record::{Task, TaskState, UserRecord},
    OperatingRedisKey, UserRecordRedisJsonPath,
};

use crate::AppState;

#[derive(Serialize, Deserialize, Debug)]
pub struct StoreTaskPayload {
    user_name: String,
    task: Task,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct RegisterRecordPayload {
    user_name: String,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct ResetUserDataPayload {
    key: String,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct GetTaskLogPayload {
    key: String,
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
                let user_data_vec: Vec<UserRecord> =
                    serde_json::from_str(&data_str).expect("Parsing `user_data` should not fail.");

                let mut user_data = user_data_vec.into_iter().next().unwrap();
                user_data
                    .task_history
                    .sort_by(|a, b| b.begin_time.cmp(&a.begin_time));

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

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateTaskPayload {
    key: String,
    state: TaskState,
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
                tracing::error!("rejection: {:?}", rejection);
                Err((rejection.status(), axum::Json(payload)))
            }
        }
    }
}

pub async fn store_task(
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

pub async fn reset_task(
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

pub async fn register_record(
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

pub async fn get_all_records(
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

pub async fn get_task_log(
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

fn perform_update_task(
    payload: UpdateTaskPayload,
    redis_client: redis::Client,
) -> Result<(), redis::RedisError> {
    let mut con = redis_client.get_connection()?;
    match con.json_get::<&std::string::String, &str, Option<String>>(
        &payload.key,
        UserRecordRedisJsonPath::Root.to_string().as_str(),
    ) {
        Ok(data_str) => match data_str {
            Some(data_str) => {
                let user_record_vec: Vec<UserRecord> = serde_json::from_str(&data_str).unwrap();

                let user_record = user_record_vec.into_iter().next().unwrap();
                if user_record.current_task.state != TaskState::End
                    && payload.state == TaskState::End
                {
                    let new_end_task = Task::generate_done_task(&user_record.current_task);
                    println!("new_end_task: {:?}", new_end_task);
                    con.json_set(
                        &payload.key,
                        UserRecordRedisJsonPath::CurrentTask.to_string().as_str(),
                        &serde_json::json!(&new_end_task),
                    )?;
                    println!("appended -> current task");
                    con.json_arr_append(
                        &payload.key,
                        UserRecordRedisJsonPath::TaskHistory.to_string().as_str(),
                        &serde_json::json!(&new_end_task),
                    )?;
                    println!("appended -> task history");

                    Ok(())
                } else {
                    // TODO: Handle the rest of the cases.
                    Ok(())
                }
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

pub async fn update_task_log(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<UpdateTaskPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match perform_update_task(payload, app_state.redis_client) {
        Ok(_) => Ok(Json(serde_json::json!({
            "status": "ok",
        }))),
        Err(err) => {
            let error_response = construct_redis_error_response(err);
            Err((StatusCode::BAD_REQUEST, Json(error_response)))
        }
    }
}
