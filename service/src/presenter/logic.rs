use std::iter::successors;

use bb8_redis::{
    bb8::Pool,
    redis::{AsyncCommands, JsonAsyncCommands},
    RedisConnectionManager,
};

use super::{
    GetTaskLogPayload, RegisterRecordPayload, RegisterSudoUserPayload, ResetUserDataPayload,
    RuntimeError, StoreTaskPayload, UpdateTaskPayload,
};
use libs::{
    record::{SudoUserRecord, Task, TaskState, UserRecord},
    OperatingRedisKey, SudoUserRecordRedisJsonPath, UserRecordRedisJsonPath,
};

pub(super) async fn perform_store_task(
    payload: StoreTaskPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<(), RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();
    match con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.user_name,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await
    {
        Ok(data_str) => match data_str {
            Some(data_str) => {
                let mut user_data: Vec<UserRecord> = serde_json::from_str(&data_str)?;
                tracing::debug!("user_data: {:?}", user_data);

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
                )
                .await?;

                tracing::debug!("appending");
                con.json_arr_append(
                    &payload.user_name,
                    UserRecordRedisJsonPath::TaskHistory.to_string().as_str(),
                    &serde_json::json!(&payload.task),
                )
                .await?;

                tracing::debug!("setting current task");
                con.json_set(
                    &payload.user_name,
                    UserRecordRedisJsonPath::CurrentTask.to_string().as_str(),
                    &serde_json::json!(&payload.task),
                )
                .await?;

                Ok(())
            }
            None => {
                tracing::debug!("non-exist record: {:?}", payload);
                Err(RuntimeError::UnprocessableEntity {
                    name: "user_name".to_string(),
                })
            }
        },
        Err(err) => {
            tracing::debug!("{:?}", err);
            Err(RuntimeError::RedisError(err))
        }
    }
}

fn generate_key(user_name: &str, id: i32) -> String {
    let id_length = successors(Some(id), |&n| (n >= 10).then_some(n / 10)).count();
    let filler_length = 4 - id_length;
    format!("{}:{}{}", user_name, "0".repeat(filler_length), id)
}

pub(super) async fn perform_register_record(
    payload: RegisterRecordPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<String, RuntimeError> {
    let new_id;

    let mut con = redis_pool.get().await.unwrap();
    match con
        .get::<&str, i32>(OperatingRedisKey::CurrentId.to_string().as_str())
        .await
    {
        Ok(current_id) => {
            new_id = current_id + 1;
            con.set("current_id", new_id).await?;
        }
        Err(_err) => {
            new_id = 0;
            con.set("current_id", 0).await?;
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
    )
    .await?;
    tracing::debug!("new user: {:?}", user_data);

    Ok(user_key)
}

pub(super) async fn perform_reset_task(
    payload: ResetUserDataPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<UserRecord, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();
    match con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await
    {
        Ok(data_str) => match data_str {
            Some(_data_str) => {
                let vec_payload_key = payload.key.split(':').collect::<Vec<&str>>();
                let user_data = UserRecord {
                    id: vec_payload_key[1].parse::<i32>().map_err(|_| {
                        RuntimeError::UnprocessableEntity {
                            name: "id".to_string(),
                        }
                    })?,
                    user_name: vec_payload_key[0].to_string(),
                    task_history: vec![],
                    current_task: Task::placeholder("reset", TaskState::Idle),
                };
                con.json_set(
                    &payload.key,
                    UserRecordRedisJsonPath::Root.to_string().as_str(),
                    &serde_json::json!(user_data),
                )
                .await?;

                Ok(user_data)
            }
            None => {
                tracing::debug!("non-exist record: {:?}", payload);
                Err(RuntimeError::UnprocessableEntity {
                    name: "key".to_string(),
                })
            }
        },
        Err(err) => {
            tracing::debug!("{:?}", err);
            Err(RuntimeError::RedisError(err))
        }
    }
}

pub(super) async fn perform_get_user_task_log(
    payload: GetTaskLogPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<UserRecord, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();
    match con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await
    {
        Ok(data_str) => match data_str {
            Some(data_str) => {
                let user_data_vec = serde_json::from_str::<Vec<UserRecord>>(&data_str)?;
                let mut user_data = user_data_vec.into_iter().next().unwrap();
                user_data
                    .task_history
                    .sort_by(|a, b| b.begin_time.cmp(&a.begin_time));

                Ok(user_data)
            }
            None => {
                tracing::debug!("non-exist record: {:?}", payload);
                Err(RuntimeError::UnprocessableEntity {
                    name: "key".to_string(),
                })
            }
        },
        Err(err) => {
            tracing::debug!("err: {:?}", err);
            Err(RuntimeError::RedisError(err))
        }
    }
}

pub(super) async fn perform_get_all_records(
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<Vec<UserRecord>, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();
    let mut keys = con
        .scan_match::<&str, std::string::String>("*:????")
        .await?;

    let mut user_records: Vec<UserRecord> = vec![];

    while let Some(key) = keys.next_item().await {
        let new_pool = redis_pool.clone();
        let mut new_con = new_pool.get().await.unwrap();

        match new_con
            .json_get::<&std::string::String, &str, Option<String>>(
                &key,
                UserRecordRedisJsonPath::Root.to_string().as_str(),
            )
            .await
        {
            Ok(data_str) => match data_str {
                Some(data_str) => {
                    let user_data: Vec<UserRecord> = serde_json::from_str(&data_str)?;
                    tracing::debug!("user_data: {:?}", user_data);

                    user_records.push(user_data.into_iter().next().unwrap());
                }
                None => {
                    // NOTE: This technically will not happen, since
                    // the keys are generated from the pre-defined pattern.
                    // TODO: Handle when there exists keys that
                    // follow the pattern but do not have the data.
                }
            },
            Err(err) => {
                tracing::debug!("{:?}", err);
                return Err(RuntimeError::RedisError(err));
            }
        }
    }

    Ok(user_records)
}

pub(super) async fn perform_update_task(
    payload: UpdateTaskPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<(), RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();
    match con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await
    {
        Ok(data_str) => match data_str {
            Some(data_str) => {
                let user_record_vec: Vec<UserRecord> = serde_json::from_str(&data_str)?;

                let user_record = user_record_vec.into_iter().next().unwrap();
                if user_record.current_task.state != TaskState::End
                    && payload.state == TaskState::End
                {
                    let new_end_task = Task::generate_done_task(&user_record.current_task);
                    tracing::debug!("new_end_task: {:?}", new_end_task);

                    con.json_set(
                        &payload.key,
                        UserRecordRedisJsonPath::CurrentTask.to_string().as_str(),
                        &serde_json::json!(&new_end_task),
                    )
                    .await?;
                    tracing::debug!("set -> current task");

                    con.json_arr_append(
                        &payload.key,
                        UserRecordRedisJsonPath::TaskHistory.to_string().as_str(),
                        &serde_json::json!(&new_end_task),
                    )
                    .await?;
                    tracing::debug!("appended -> task history");

                    Ok(())
                } else {
                    // TODO: Handle the rest of the cases.
                    Ok(())
                }
            }
            None => {
                tracing::debug!("non-exist record: {:?}", payload);
                Err(RuntimeError::UnprocessableEntity {
                    name: "key".to_string(),
                })
            }
        },
        Err(err) => {
            tracing::debug!("{:?}", err);
            Err(RuntimeError::RedisError(err))
        }
    }
}

pub(super) async fn perform_register_sudo_user(
    payload: RegisterSudoUserPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<(), RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();
    match con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.user_name,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await
    {
        Ok(data_str) => match data_str {
            Some(_data_str) => {
                tracing::debug!("user already exists: {:?}", payload);

                Err(RuntimeError::UnprocessableEntity {
                    name: "user_name".to_string(),
                })
            }
            None => {
                tracing::debug!("registering sudo user: {:?}", payload);

                let id;
                match con
                    .get::<&str, i32>(OperatingRedisKey::CurrentId.to_string().as_str())
                    .await
                {
                    Ok(current_id) => {
                        id = current_id + 1;
                        con.set("current_id", id).await?;
                    }
                    Err(_err) => {
                        id = 0;
                        con.set("current_id", 0).await?;
                    }
                }

                let user_data = SudoUserRecord {
                    id,
                    user_name: payload.user_name.clone(),
                    published_tasks: vec![],
                };
                let user_key = generate_key(&payload.user_name, id);
                con.json_set(
                    user_key,
                    SudoUserRecordRedisJsonPath::Root.to_string().as_str(),
                    &serde_json::json!(user_data),
                )
                .await?;

                Ok(())
            }
        },
        Err(err) => {
            tracing::debug!("{:?}", err);
            Err(RuntimeError::RedisError(err))
        }
    }
}
