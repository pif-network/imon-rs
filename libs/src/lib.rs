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
pub enum OperatingRedisKey {
    #[strum(serialize = "current_id")]
    CurrentId,
}
