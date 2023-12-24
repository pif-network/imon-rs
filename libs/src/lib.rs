use strum_macros::Display;

pub mod record;

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
pub enum OperatingRedisKey {
    CurrentId,
}

#[derive(Debug, Display)]
#[strum(serialize_all = "snake_case")]
pub enum UserType {
    User,
    #[strum(serialize = "sudo")]
    SudoUser,
}
