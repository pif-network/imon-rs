use std::iter::successors;

use redis::{Commands, JsonCommands};

use super::{
    GetTaskLogPayload, RegisterRecordPayload, ResetUserDataPayload, StoreTaskPayload,
    UpdateTaskPayload,
};
use libs::{
    record::{Task, TaskState, UserRecord},
    OperatingRedisKey, UserRecordRedisJsonPath,
};

pub(super) fn perform_store_task(
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

fn generate_key(user_name: &str, id: i32) -> String {
    let id_length = successors(Some(id), |&n| (n >= 10).then(|| n / 10)).count();
    let filler_length = 4 - id_length;
    format!("{}:{}{}", user_name, "0".repeat(filler_length), id)
}

pub(super) fn perform_register_record(
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

pub(super) fn perform_reset_task(
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

pub(super) fn perform_get_user_task_log(
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

pub(super) fn perform_get_all_records(
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

pub(super) fn perform_update_task(
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
