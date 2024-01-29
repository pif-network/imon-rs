use axum::{
    async_trait,
    extract::{rejection::JsonRejection, FromRequest, Request as AxumExtractRequest, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use super::{
    construct_err_resp_invalid_incoming_json,
    logic::{
        perform_create_task, perform_get_all_user_records, perform_get_user_record,
        perform_register_record, perform_reset_record, perform_sudo_create_task,
        perform_sudo_get_record, perform_sudo_register_record, perform_sudo_reset_record,
        perform_update_task,
    },
    RpcPayloadType, RuntimeError, SudoUserRpcEventPayload, SudoUserRpcRequest, UserRpcEventPayload,
    UserRpcRequest,
};
use crate::{presenter::logic::perform_get_all_sudo_records, AppState};
use libs::payload::{
    GetSingleRecordPayload, RegisterRecordPayload, ResetRecordPayload, StoreTaskPayload,
    UpdateTaskPayload,
};

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
                tracing::error!("{:?}", rejection);
                let err_resp = construct_err_resp_invalid_incoming_json(&rejection);
                Err(err_resp)
            }
        }
    }
}

pub async fn create_task(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<StoreTaskPayload>,
) -> Result<impl IntoResponse, RuntimeError> {
    perform_create_task(payload, app_state.redis_pool).await?;
    Ok(Json(serde_json::json!({
    "status": "ok",
    })))
}

pub async fn reset_task(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<ResetRecordPayload>,
) -> Result<impl IntoResponse, RuntimeError> {
    let user_data = perform_reset_record(payload, app_state.redis_pool).await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "data": {
            "user_data": user_data,
        }
    })))
}

pub async fn register_record(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<RegisterRecordPayload>,
) -> Result<impl IntoResponse, RuntimeError> {
    let user_key = perform_register_record(payload, app_state.redis_pool).await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "data": {
            "user_key": user_key,
        }
    })))
}

pub async fn get_all_user_records(
    State(app_state): State<AppState>,
) -> Result<impl IntoResponse, RuntimeError> {
    let user_records = perform_get_all_user_records(app_state.redis_pool).await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "data": {
            "user_records": user_records,
        }
    })))
}

pub async fn get_user_record(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<GetSingleRecordPayload>,
) -> Result<impl IntoResponse, RuntimeError> {
    let task_log = perform_get_user_record(payload, app_state.redis_pool).await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "data": {
            "task_log": task_log,
        }
    })))
}

pub async fn update_task_log(
    State(app_state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<UpdateTaskPayload>,
) -> Result<impl IntoResponse, RuntimeError> {
    perform_update_task(payload, app_state.redis_pool).await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
    })))
}

pub async fn user_rpc(
    State(app_state): State<AppState>,
    ValidatedJson(request): ValidatedJson<UserRpcRequest>,
) -> Result<impl IntoResponse, RuntimeError> {
    tracing::debug!("request: {:?}", request);
    match request.metadata.of {
        RpcPayloadType::User => match request.payload {
            UserRpcEventPayload::RegisterRecord(payload) => {
                perform_register_record(payload, app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                })))
            }
            UserRpcEventPayload::AddTask(payload) => {
                perform_create_task(payload, app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                })))
            }
            UserRpcEventPayload::UpdateTask(payload) => {
                perform_update_task(payload, app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                })))
            }
            UserRpcEventPayload::ResetRecord(payload) => {
                perform_reset_record(payload, app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                })))
            }
            UserRpcEventPayload::GetSingleRecord(payload) => {
                let record = perform_get_user_record(payload, app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                    "data": {
                        "task_log": record,
                }
                })))
            }
            UserRpcEventPayload::GetAllRecord => {
                let records = perform_get_all_user_records(app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                    "data": {
                        "user_records": records,
                }
                })))
            }
        },
        RpcPayloadType::Sudo => Err(RuntimeError::UnprocessableEntity {
            name: "metadata.of".to_string(),
        })?,
    }
}

pub async fn sudo_user_rpc(
    State(app_state): State<AppState>,
    ValidatedJson(request): ValidatedJson<SudoUserRpcRequest>,
) -> Result<impl IntoResponse, RuntimeError> {
    tracing::debug!("request: {:?}", request);
    match request.metadata.of {
        RpcPayloadType::Sudo => match request.payload {
            SudoUserRpcEventPayload::RegisterRecord(payload) => {
                perform_sudo_register_record(payload, app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                })))
            }
            SudoUserRpcEventPayload::AddTask(payload) => {
                perform_sudo_create_task(payload, app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                })))
            }
            SudoUserRpcEventPayload::ResetRecord(payload) => {
                perform_sudo_reset_record(payload, app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                })))
            }
            SudoUserRpcEventPayload::GetSingleRecord(payload) => {
                let record = perform_sudo_get_record(payload, app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                    "data": record
                })))
            }
            SudoUserRpcEventPayload::GetAllRecord => {
                let records = perform_get_all_sudo_records(app_state.redis_pool).await?;
                Ok(Json(serde_json::json!({
                    "status": "ok",
                    "data": {
                        "user_records": records,
                }
                })))
            }
        },
        RpcPayloadType::User => Err(RuntimeError::UnprocessableEntity {
            name: "metadata.of".to_string(),
        })?,
    }
}
