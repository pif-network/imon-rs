use axum::{extract::rejection::JsonRejection, http::StatusCode, response::IntoResponse, Json};
use bb8_redis::redis;
use serde::{Deserialize, Serialize};

use imon_derive::TryFromPayload;
use libs::record::{STask, Task, TaskState};

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
pub struct ResetRecordPayload {
    key: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetSingleRecordPayload {
    key: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateTaskPayload {
    key: String,
    state: TaskState,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StoreSTaskPayload {
    key: String,
    task: STask,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum RpcPayloadType {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "sudo")]
    Sudo,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum SudoUserRpcEventType {
    #[serde(rename = "register")]
    RegisterRecord,
    #[serde(rename = "add_task")]
    AddTask,
    #[serde(rename = "reset_record")]
    ResetRecord,
    #[serde(rename = "get_single_record")]
    GetSingleRecord,
}

#[derive(Serialize, Deserialize, Debug, TryFromPayload)]
#[serde(untagged)]
pub enum SudoUserRpcEventPayload {
    #[serde(rename = "register")]
    RegisterRecord(RegisterRecordPayload),
    #[serde(rename = "add_task")]
    AddTask(StoreSTaskPayload),
    #[serde(rename = "reset_record")]
    ResetRecord(ResetRecordPayload),
    #[serde(rename = "get_single_record")]
    GetSingleRecord(GetSingleRecordPayload),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcPayloadMetadata {
    of: RpcPayloadType,
    event_type: SudoUserRpcEventType,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SudoUserRpcRequest {
    metadata: RpcPayloadMetadata,
    payload: SudoUserRpcEventPayload,
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

impl IntoResponse for RuntimeError {
    fn into_response(self) -> axum::http::Response<axum::body::Body> {
        match self {
            RuntimeError::RedisError(err) => {
                let err_payload = construct_err_payload_redis(err);
                (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(err_payload)).into_response()
            }
            RuntimeError::SerdeError(err) => {
                let err_payload = construct_err_payload_de_upstream_data(err);
                (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(err_payload)).into_response()
            }
            RuntimeError::UnprocessableEntity { name } => {
                let err_payload = construct_err_payload_unprocessable_entity(name);
                (StatusCode::UNPROCESSABLE_ENTITY, axum::Json(err_payload)).into_response()
            }
        }
    }
}

fn construct_err_payload_unprocessable_entity(name: String) -> serde_json::Value {
    serde_json::json!({
        "status": "error",
        "message": "Unprocessable entity",
        "field": name,
    })
}

fn construct_err_payload_redis(err: redis::RedisError) -> serde_json::Value {
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

fn construct_err_payload_de_upstream_data(err: serde_json::Error) -> serde_json::Value {
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
