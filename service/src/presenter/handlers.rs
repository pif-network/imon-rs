use axum::{
    async_trait,
    extract::{rejection::JsonRejection, FromRequest, Request as AxumExtractRequest, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use super::{
    construct_json_error_response, construct_redis_error_response,
    logic::{
        perform_get_all_records, perform_get_user_task_log, perform_register_record,
        perform_reset_task, perform_store_task, perform_update_task,
    },
    GetTaskLogPayload, RegisterRecordPayload, ResetUserDataPayload, StoreTaskPayload,
    UpdateTaskPayload,
};
use crate::AppState;

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
                Err((rejection.status(), Json(payload)))
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
