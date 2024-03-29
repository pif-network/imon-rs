use serde::{Deserialize, Serialize};
use strum_macros::Display;

pub mod payload;
pub mod record;

#[derive(Serialize, Deserialize, Debug)]
pub struct OperatingInfo {
    pub latest_record_id: i32,
    pub latest_sudo_record_id: i32,
    pub user_list: Vec<String>,
    pub sudo_user_list: Vec<String>,
}

#[derive(Debug, Display)]
pub enum OperatingInfoRedisJsonPath {
    #[strum(serialize = "$")]
    Root,
    #[strum(serialize = "$.latest_record_id")]
    LatestRecordId,
    #[strum(serialize = "$.latest_sudo_record_id")]
    LatestSudoRecordId,
    #[strum(serialize = "$.user_list")]
    UserList,
    #[strum(serialize = "$.sudo_user_list")]
    SudoUserList,
}

#[derive(Debug, Display)]
#[strum(serialize_all = "snake_case")]
pub enum OperatingRedisKey {
    CurrentId,
    OperatingInfo,
}

#[derive(Debug, Display)]
pub enum UserRecordRedisJsonPath {
    #[strum(serialize = "$")]
    Root,
    #[strum(serialize = "$.task_history")]
    TaskHistory,
    #[strum(serialize = "$.current_task")]
    CurrentTask,
}

#[derive(Debug, Display)]
pub enum SudoUserRecordRedisJsonPath {
    #[strum(serialize = "$")]
    Root,
    #[strum(serialize = "$.published_tasks")]
    PublishedTasks,
}

#[derive(Debug, Display)]
#[strum(serialize_all = "snake_case")]
pub enum UserType {
    User,
    #[strum(serialize = "sudo")]
    SudoUser,
}
