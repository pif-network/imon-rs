use std::iter::successors;

use bb8_redis::{
    bb8::Pool,
    redis::{AsyncCommands, JsonAsyncCommands},
    RedisConnectionManager,
};

use super::RuntimeError;
use libs::{
    payload::{
        GetSingleRecordPayload, RegisterRecordPayload, ResetRecordPayload, StoreSTaskPayload,
        StoreTaskPayload, UpdateTaskPayload,
    },
    record::{STask, SudoUserRecord, Task, TaskState, UserRecord},
    OperatingInfoRedisJsonPath, OperatingRedisKey, SudoUserRecordRedisJsonPath,
    UserRecordRedisJsonPath, UserType,
};

pub(super) async fn perform_create_task(
    payload: StoreTaskPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<(), RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let Some(data_str) = con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await?
    else {
        tracing::debug!("non-exist record: {:?}", payload);
        return Err(RuntimeError::UnprocessableEntity {
            name: "payload.key".to_string(),
        });
    };

    let mut user_data_vec: Vec<UserRecord> = serde_json::from_str(&data_str)?;

    // Remove the latest task from the history
    // to append the updated version later.
    if user_data_vec[0].current_task.state == TaskState::Begin
        || user_data_vec[0].current_task.state == TaskState::Break
        || user_data_vec[0].current_task.state == TaskState::Back
    {
        user_data_vec[0].task_history.pop();
    };

    let task_history = user_data_vec.into_iter().next().unwrap().task_history;
    con.json_set(
        &payload.key,
        UserRecordRedisJsonPath::TaskHistory.to_string().as_str(),
        &serde_json::json!(task_history),
    )
    .await?;

    tracing::debug!("appending");
    con.json_arr_append(
        &payload.key,
        UserRecordRedisJsonPath::TaskHistory.to_string().as_str(),
        &serde_json::json!(&payload.task),
    )
    .await?;

    tracing::debug!("setting current task");
    con.json_set(
        &payload.key,
        UserRecordRedisJsonPath::CurrentTask.to_string().as_str(),
        &serde_json::json!(&payload.task),
    )
    .await?;

    Ok(())
}

pub(super) async fn perform_register_record(
    payload: RegisterRecordPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<String, RuntimeError> {
    let id = get_new_record_id(UserType::User, redis_pool.clone()).await?;
    let user_key = generate_key(UserType::User, &payload.user_name, id);
    let user_data = UserRecord {
        id,
        user_name: payload.user_name,
        task_history: vec![],
        current_task: Task::placeholder("initialised", TaskState::Placeholder),
    };

    let mut con = redis_pool.get().await.unwrap();
    con.json_set(
        &user_key,
        UserRecordRedisJsonPath::Root.to_string().as_str(),
        &serde_json::json!(user_data),
    )
    .await?;
    tracing::debug!("new_user: {:?}", user_data.user_name);

    store_to_record_list(UserType::User, &user_data.user_name, redis_pool.clone()).await?;

    Ok(user_key)
}

pub(super) async fn perform_reset_record(
    payload: ResetRecordPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<UserRecord, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let key_exists = con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await?
        .is_some();
    if !key_exists {
        tracing::debug!("non-exist record: {:?}", payload);
        return Err(RuntimeError::UnprocessableEntity {
            name: "payload.key".to_string(),
        });
    }

    let vec_payload_key = payload.key.split(':').collect::<Vec<&str>>();
    let user_data = UserRecord {
        id: vec_payload_key[1]
            .parse::<i32>()
            .map_err(|_| RuntimeError::UnprocessableEntity {
                name: "id".to_string(),
            })?,
        user_name: vec_payload_key[0].to_string(),
        task_history: vec![],
        current_task: Task::placeholder("reset", TaskState::Placeholder),
    };
    con.json_set(
        &payload.key,
        UserRecordRedisJsonPath::Root.to_string().as_str(),
        &serde_json::json!(user_data),
    )
    .await?;

    Ok(user_data)
}

pub(super) async fn perform_get_user_record(
    payload: GetSingleRecordPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<UserRecord, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let Some(data_str) = con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await?
    else {
        tracing::debug!("non-exist record: {:?}", payload);
        return Err(RuntimeError::UnprocessableEntity {
            name: "payload.key".to_string(),
        });
    };

    let user_data_vec = serde_json::from_str::<Vec<UserRecord>>(&data_str)?;
    let mut user_data = user_data_vec.into_iter().next().unwrap();
    user_data
        .task_history
        .sort_by(|a, b| b.begin_time.cmp(&a.begin_time));

    Ok(user_data)
}

pub(super) async fn perform_get_all_user_records(
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<Vec<UserRecord>, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();
    let keys_resp_str: String = con
        .json_get(
            OperatingRedisKey::OperatingInfo.to_string().as_str(),
            OperatingInfoRedisJsonPath::UserList.to_string().as_str(),
        )
        .await?;
    let keys_resp = serde_json::from_str::<Vec<Vec<String>>>(&keys_resp_str)?;
    let keys = keys_resp.into_iter().next().unwrap();

    let mut user_records: Vec<UserRecord> = vec![];

    for key in keys {
        let Some(data_str) = con
            .json_get::<&std::string::String, &str, Option<String>>(
                &key,
                UserRecordRedisJsonPath::Root.to_string().as_str(),
            )
            .await?
        else {
            // NOTE: This technically will not happen, since
            // the keys are generated from the pre-defined pattern.
            // TODO: Handle when there exists keys that
            // follow the pattern but do not have the data.
            panic!("invalid record found: {:?}", key);
        };

        let user_data_vec: Vec<UserRecord> = serde_json::from_str(&data_str)?;
        let user_data = user_data_vec.into_iter().next().unwrap();
        tracing::debug!("retrieved_user_data: {:?}", user_data.user_name);

        user_records.push(user_data);
    }

    Ok(user_records)
}

pub(super) async fn perform_get_all_sudo_records(
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<Vec<SudoUserRecord>, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();
    let keys_resp_str: String = con
        .json_get(
            OperatingRedisKey::OperatingInfo.to_string().as_str(),
            OperatingInfoRedisJsonPath::SudoUserList
                .to_string()
                .as_str(),
        )
        .await?;
    let keys_resp = serde_json::from_str::<Vec<Vec<String>>>(&keys_resp_str)?;
    let keys = keys_resp.into_iter().next().unwrap();

    let mut sudo_records: Vec<SudoUserRecord> = vec![];

    for key in keys {
        let Some(data_str) = con
            .json_get::<&std::string::String, &str, Option<String>>(
                &key,
                SudoUserRecordRedisJsonPath::Root.to_string().as_str(),
            )
            .await?
        else {
            // NOTE: This technically will not happen, since
            // the keys are generated from the pre-defined pattern.
            // TODO: Handle when there exists keys that
            // follow the pattern but do not have the data.
            panic!("invalid record found: {:?}", key);
        };

        let sudo_user_data_vec: Vec<SudoUserRecord> = serde_json::from_str(&data_str)?;
        let sudo_user_data = sudo_user_data_vec.into_iter().next().unwrap();
        tracing::debug!("retrieved_sudo_user: {:?}", sudo_user_data.user_name);

        sudo_records.push(sudo_user_data);
    }

    Ok(sudo_records)
}

// pub(super) async fn perform_get_all_user_records(
//     redis_pool: Pool<RedisConnectionManager>,
// ) -> Result<Vec<UserRecord>, RuntimeError> {
//     let mut con = redis_pool.get().await.unwrap();
//     let mut keys = con
//         .scan_match::<&str, std::string::String>("user:*:????")
//         .await?;
//
//     let mut user_records: Vec<UserRecord> = vec![];
//
//     while let Some(key) = keys.next_item().await {
//         let mut new_con = redis_pool.get().await.unwrap();
//
//         let Some(data_str) = new_con
//             .json_get::<&std::string::String, &str, Option<String>>(
//                 &key,
//                 UserRecordRedisJsonPath::Root.to_string().as_str(),
//             )
//             .await?
//         else {
//             // NOTE: This technically will not happen, since
//             // the keys are generated from the pre-defined pattern.
//             // TODO: Handle when there exists keys that
//             // follow the pattern but do not have the data.
//             panic!("invalid record found: {:?}", key);
//         };
//
//         let user_data: Vec<UserRecord> = serde_json::from_str(&data_str)?;
//         tracing::debug!("user_data: {:?}", user_data);
//
//         user_records.push(user_data.into_iter().next().unwrap());
//     }
//
//     Ok(user_records)
// }

pub(super) async fn perform_update_task(
    payload: UpdateTaskPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<(), RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let Some(data_str) = con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await?
    else {
        tracing::debug!("non-exist record: {:?}", payload);
        return Err(RuntimeError::UnprocessableEntity {
            name: "payload.key".to_string(),
        });
    };

    let user_record_vec: Vec<UserRecord> = serde_json::from_str(&data_str)?;
    let user_record = user_record_vec.into_iter().next().unwrap();

    if user_record.current_task.state != TaskState::End && payload.state == TaskState::End {
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

pub(super) async fn perform_sudo_register_record(
    payload: RegisterRecordPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<(), RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let id = get_new_record_id(UserType::SudoUser, redis_pool.clone()).await?;
    con.set(OperatingRedisKey::CurrentId.to_string(), id)
        .await?;

    let user_data = SudoUserRecord {
        id,
        user_name: payload.user_name.clone(),
        published_tasks: vec![],
    };
    let user_key = generate_key(UserType::SudoUser, &payload.user_name, id);
    con.json_set(
        user_key,
        SudoUserRecordRedisJsonPath::Root.to_string().as_str(),
        &serde_json::json!(user_data),
    )
    .await?;
    tracing::debug!("new_sudo_user: {:?}", user_data.user_name);

    store_to_record_list(UserType::SudoUser, &user_data.user_name, redis_pool.clone()).await?;

    Ok(())
}

pub(super) async fn perform_sudo_create_task(
    payload: StoreSTaskPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<(), RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let Some(_data_str) = con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            SudoUserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await?
    else {
        tracing::debug!("non-exist record: {:?}", payload);
        return Err(RuntimeError::UnprocessableEntity {
            name: "payload.key".to_string(),
        });
    };

    let new_task = STask {
        name: payload.task.name,
        description: payload.task.description,
        created_at: chrono::offset::Local::now().naive_local(),
    };

    tracing::debug!("appending");
    con.json_arr_append(
        &payload.key,
        SudoUserRecordRedisJsonPath::PublishedTasks
            .to_string()
            .as_str(),
        &serde_json::json!(new_task),
    )
    .await?;

    Ok(())
}

pub(super) async fn perform_sudo_reset_record(
    payload: ResetRecordPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<SudoUserRecord, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let key_exists = con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            SudoUserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await?
        .is_some();
    if !key_exists {
        tracing::debug!("non-exist record: {:?}", payload);
        return Err(RuntimeError::UnprocessableEntity {
            name: "payload.key".to_string(),
        });
    }

    let vec_payload_key = payload.key.split(':').collect::<Vec<&str>>();
    let user_data = SudoUserRecord {
        id: vec_payload_key[2]
            .parse::<i32>()
            // NOTE: Although it may appears that this check is obsolete,
            // it is still necessary to ensure that uses would only get
            // responses from correct key, even from the db.
            .map_err(|_| RuntimeError::UnprocessableEntity {
                name: "payload.key".to_string(),
            })?,
        user_name: vec_payload_key[1].to_string(),
        published_tasks: vec![],
    };
    con.json_set(
        &payload.key,
        SudoUserRecordRedisJsonPath::Root.to_string().as_str(),
        &serde_json::json!(user_data),
    )
    .await?;

    Ok(user_data)
}

pub(super) async fn perform_sudo_get_record(
    payload: GetSingleRecordPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<SudoUserRecord, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let Some(data_str) = con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.key,
            SudoUserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await?
    else {
        tracing::debug!("non-exist record: {:?}", payload);
        return Err(RuntimeError::UnprocessableEntity {
            name: "payload.key".to_string(),
        });
    };

    let user_data_vec = serde_json::from_str::<Vec<SudoUserRecord>>(&data_str)?;
    let mut user_data = user_data_vec.into_iter().next().unwrap();
    user_data
        .published_tasks
        .sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(user_data)
}

fn generate_key(user_type: UserType, user_name: &str, id: i32) -> String {
    let id_length = successors(Some(id), |&n| (n >= 10).then_some(n / 10)).count();
    let filler_length = 4 - id_length;
    format!(
        "{}:{}:{}{}",
        user_type,
        user_name,
        "0".repeat(filler_length),
        id
    )
}

/// Get new incremented ID when creating a new record.
async fn get_new_record_id(
    user_type: UserType,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<i32, RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let id_path = match user_type {
        UserType::User => OperatingInfoRedisJsonPath::LatestRecordId.to_string(),
        UserType::SudoUser => OperatingInfoRedisJsonPath::LatestSudoRecordId.to_string(),
    };

    match con
        .json_get(OperatingRedisKey::OperatingInfo.to_string(), &id_path)
        .await?
    {
        Some(id) => Ok(id),
        _ => Ok(0),
    }
}

/// Store newly created record's name to an according list.
async fn store_to_record_list(
    user_type: UserType,
    user_name: &str,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<(), RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let key = match user_type {
        UserType::User => OperatingInfoRedisJsonPath::UserList.to_string(),
        UserType::SudoUser => OperatingInfoRedisJsonPath::SudoUserList.to_string(),
    };

    con.json_arr_append(
        OperatingRedisKey::OperatingInfo.to_string(),
        &key,
        &user_name,
    )
    .await?;

    Ok(())
}
