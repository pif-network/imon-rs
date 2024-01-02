use std::iter::successors;

use bb8_redis::{
    bb8::Pool,
    redis::{AsyncCommands, JsonAsyncCommands},
    RedisConnectionManager,
};

use super::{
    GetSingleRecordPayload, RegisterRecordPayload, ResetRecordPayload, RuntimeError,
    StoreSTaskPayload, StoreTaskPayload, UpdateTaskPayload,
};
use libs::{
    record::{STask, SudoUserRecord, Task, TaskState, UserRecord},
    OperatingRedisKey, SudoUserRecordRedisJsonPath, UserRecordRedisJsonPath, UserType,
};

pub(super) async fn perform_create_task(
    payload: StoreTaskPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<(), RuntimeError> {
    let mut con = redis_pool.get().await.unwrap();

    let Some(data_str) = con
        .json_get::<&std::string::String, &str, Option<String>>(
            &payload.user_name,
            UserRecordRedisJsonPath::Root.to_string().as_str(),
        )
        .await?
    else {
        tracing::debug!("non-exist record: {:?}", payload);
        return Err(RuntimeError::UnprocessableEntity {
            name: "key".to_string(),
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
        &payload.user_name,
        UserRecordRedisJsonPath::TaskHistory.to_string().as_str(),
        &serde_json::json!(task_history),
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

pub(super) async fn perform_register_record(
    payload: RegisterRecordPayload,
    redis_pool: Pool<RedisConnectionManager>,
) -> Result<String, RuntimeError> {
    let id = get_user_id(redis_pool.clone()).await;
    let user_key = generate_key(UserType::User, &payload.user_name, id);
    let user_data = UserRecord {
        id,
        user_name: payload.user_name,
        task_history: vec![],
        current_task: Task::placeholder("initialised", TaskState::Idle),
    };

    let mut con = redis_pool.get().await.unwrap();
    con.json_set(
        &user_key,
        UserRecordRedisJsonPath::Root.to_string().as_str(),
        &serde_json::json!(user_data),
    )
    .await?;
    tracing::debug!("new user: {:?}", user_data);

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
            name: "key".to_string(),
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
            name: "key".to_string(),
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
    let mut keys = con
        .scan_match::<&str, std::string::String>("user:*:????")
        .await?;

    let mut user_records: Vec<UserRecord> = vec![];

    while let Some(key) = keys.next_item().await {
        let mut new_con = redis_pool.get().await.unwrap();

        let Some(data_str) = new_con
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

        let user_data: Vec<UserRecord> = serde_json::from_str(&data_str)?;
        tracing::debug!("user_data: {:?}", user_data);

        user_records.push(user_data.into_iter().next().unwrap());
    }

    Ok(user_records)
}

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
            name: "key".to_string(),
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

    let id = get_user_id(redis_pool.clone()).await;
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
            name: "key".to_string(),
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
            name: "key".to_string(),
        });
    }

    let vec_payload_key = payload.key.split(':').collect::<Vec<&str>>();
    let user_data = SudoUserRecord {
        id: vec_payload_key[2]
            .parse::<i32>()
            // NOTE: Although it may appears that this check is obsolete,
            // it is still necessary to ensure that uses would only get
            // responses from correct key.
            .map_err(|_| RuntimeError::UnprocessableEntity {
                name: "key".to_string(),
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
            name: "key".to_string(),
        });
    };

    let user_data_vec = serde_json::from_str::<Vec<SudoUserRecord>>(&data_str)?;
    let user_data = user_data_vec.into_iter().next().unwrap();
    // TODO: Sort the tasks by creation time.
    // user_data
    //     .published_tasks
    //     .sort_by(|a, b| b.created_at.cmp(&a.created_at));

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

async fn get_user_id(redis_pool: Pool<RedisConnectionManager>) -> i32 {
    let mut con = redis_pool.get().await.unwrap();
    match con
        .get::<&str, i32>(OperatingRedisKey::CurrentId.to_string().as_str())
        .await
    {
        Ok(current_id) => current_id + 1,
        Err(_err) => 0,
    }
}
