use serde::{Deserialize, Serialize};

use crate::record::{Task, TaskState};

#[derive(Serialize, Deserialize, Debug)]
pub struct StoreTaskPayload {
    pub key: String,
    pub task: Task,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegisterRecordPayload {
    pub user_name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ResetRecordPayload {
    pub key: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetSingleRecordPayload {
    pub key: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateTaskPayload {
    pub key: String,
    pub state: TaskState,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct STaskIn {
    pub name: String,
    pub description: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct StoreSTaskPayload {
    pub key: String,
    pub task: STaskIn,
}
