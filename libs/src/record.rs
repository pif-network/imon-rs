use chrono::NaiveDateTime;
use redis::FromRedisValue;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum TaskState {
    Begin,
    Break,
    Back,
    End,
    Idle,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Task {
    pub name: String,
    pub state: TaskState,
    pub begin_time: NaiveDateTime,
    pub end_time: NaiveDateTime,
    pub duration: i64,
}

impl Default for Task {
    fn default() -> Self {
        Task {
            name: String::new(),
            state: TaskState::Idle,
            begin_time: chrono::offset::Local::now().naive_local(),
            end_time: chrono::offset::Local::now().naive_local(),
            duration: 0,
        }
    }
}

impl Task {
    pub fn placeholder(name: &str, state: TaskState) -> Self {
        Task {
            name: name.to_string(),
            state,
            ..Task::default()
        }
    }

    pub fn generate_begin_task(name: String) -> Self {
        Task {
            name,
            state: TaskState::Begin,
            ..Task::default()
        }
    }

    pub fn generate_break_task(latest_task: &Task) -> Self {
        let duration = Task::calculate_duration(latest_task);
        Task {
            name: latest_task.name.clone(),
            state: TaskState::Break,
            duration,
            end_time: chrono::offset::Local::now().naive_local(),
            ..*latest_task
        }
    }

    pub fn generate_back_task(latest_task: &Task) -> Self {
        Task {
            name: latest_task.name.clone(),
            state: TaskState::Back,
            begin_time: Task::default().begin_time,
            ..*latest_task
        }
    }

    pub fn generate_done_task(latest_task: &Task) -> Self {
        if latest_task.state == TaskState::Break {
            Task {
                name: latest_task.name.clone(),
                state: TaskState::End,
                ..*latest_task
            }
        } else if latest_task.state == TaskState::Back {
            let duration = Task::calculate_duration(latest_task) + latest_task.duration;
            Task {
                name: latest_task.name.clone(),
                state: TaskState::End,
                duration,
                begin_time: latest_task.begin_time,
                ..Task::default()
            }
        } else {
            let duration = Task::calculate_duration(latest_task);
            Task {
                name: latest_task.name.clone(),
                state: TaskState::End,
                duration,
                ..Task::default()
            }
        }
    }

    fn calculate_duration(&self) -> i64 {
        let duration = chrono::offset::Local::now().naive_local() - self.begin_time;
        duration.num_seconds()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UserRecord {
    pub id: i32,
    pub user_name: String,
    pub task_history: Vec<Task>,
    pub current_task: Task,
}

impl FromRedisValue for UserRecord {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<UserRecord> {
        match *v {
            redis::Value::Data(ref bytes) => {
                let user_data: UserRecord = serde_json::from_slice(bytes)?;
                Ok(user_data)
            }
            _ => Err((redis::ErrorKind::TypeError, "Invalid type").into()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct STask {
    pub id: i32,
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SudoUserRecord {
    pub id: i32,
    pub user_name: String,
    pub published_tasks: Vec<STask>,
}
