use axum::{extract::rejection::JsonRejection, http::StatusCode, Json};
use bb8_redis::redis;
use serde::{Deserialize, Serialize};

use libs::record::{Task, TaskState};

pub mod handlers;
pub mod logic;

#[derive(Serialize, Deserialize, Debug)]
pub struct StoreTaskPayload {
    user_name: String,
    task: Task,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegisterRecordPayload {
    user_name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ResetUserDataPayload {
    key: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetTaskLogPayload {
    key: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateTaskPayload {
    key: String,
    state: TaskState,
}

#[derive(thiserror::Error, Debug)]
pub enum RuntimeError {
    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    #[error("JSON error: {0}")]
    SerdeError(#[from] serde_json::Error),

    #[error("Invalid payload")]
    UnprocessableEntity { name: String },
}

fn construct_error_response(err: RuntimeError) -> serde_json::Value {
    match err {
        RuntimeError::RedisError(err) => construct_err_resp_redis(err),
        RuntimeError::SerdeError(err) => construct_err_resp_de_upstream_data(err),
        RuntimeError::UnprocessableEntity { name } => construct_err_resp_unprocessable_entity(name),
    }
}

fn construct_err_resp_unprocessable_entity(name: String) -> serde_json::Value {
    serde_json::json!({
        "status": "error",
        "message": "Unprocessable entity",
        "field": name,
    })
}

fn construct_err_resp_redis(err: redis::RedisError) -> serde_json::Value {
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

fn construct_err_resp_de_upstream_data(err: serde_json::Error) -> serde_json::Value {
    serde_json::json!({
        "status": "error",
        "message": err.to_string(),
    })
}

fn construct_err_resp_invalid_incoming_json(
    err: &JsonRejection,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    match err {
        case @ JsonRejection::JsonDataError(_) => {
            let p = serde_json::json!({
                "status": "error",
                "message": "Invalid JSON",
                "error": format!("{:?}", case.body_text()),
            });
            (StatusCode::BAD_REQUEST, Json(p))
        }
        _ => {
            let p = serde_json::json!({
                "status": "error",
                "message": "Unknown error",
            });
            (StatusCode::BAD_REQUEST, Json(p))
        }
    }
}
