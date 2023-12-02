use std::path::PathBuf;
use std::{
    fs,
    io::{Read, Write},
};

use chrono;
use chrono::NaiveDateTime;
use clap::{Parser, Subcommand};
use reqwest;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Serialize, Deserialize, Debug)]
struct TaskResponse {
    status: String,
    message: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct AuthResponseData {
    user_key: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AuthResponse {
    status: String,
    data: AuthResponseData,
}

#[derive(Subcommand)]
enum Commands {
    /// What are you working on?
    On {
        name: Option<String>,
    },
    /// Take a break.
    Break,
    /// Go back to work.
    Back,
    /// Signals that you have done working on registered task.
    Done,
    Check,
    /// Register yourself.
    Auth {
        user_name: Option<String>,
    },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum TaskState {
    Begin,
    Break,
    Back,
    End,
}

#[derive(Serialize, Deserialize, Debug)]
struct Task {
    name: String,
    state: TaskState,
    begin_time: NaiveDateTime,
    end_time: NaiveDateTime,
    duration: i64,
}

#[derive(Serialize, Deserialize, Debug)]
struct UserData {
    id: i32,
    task_history: Vec<Task>,
    current_task: Task,
}

impl Default for Task {
    fn default() -> Self {
        let now = chrono::offset::Local::now().naive_local();
        Task {
            name: String::new(),
            state: TaskState::End,
            begin_time: now,
            end_time: now,
            duration: 0,
        }
    }
}

impl Task {
    fn generate_begin_task(name: String) -> Self {
        Task {
            name,
            state: TaskState::Begin,
            ..Task::default()
        }
    }

    fn generate_break_task(latest_task: &Task) -> Self {
        let duration = Task::calculate_duration(&latest_task);
        Task {
            name: latest_task.name.clone(),
            state: TaskState::Break,
            duration,
            end_time: chrono::offset::Local::now().naive_local(),
            ..*latest_task
        }
    }

    fn generate_back_task(latest_task: &Task) -> Self {
        Task {
            name: latest_task.name.clone(),
            state: TaskState::Back,
            begin_time: Task::default().begin_time,
            ..*latest_task
        }
    }

    fn generate_done_task(latest_task: &Task) -> Self {
        if latest_task.state == TaskState::Break {
            Task {
                name: latest_task.name.clone(),
                state: TaskState::End,
                ..*latest_task
            }
        } else if latest_task.state == TaskState::Back {
            let duration = Task::calculate_duration(&latest_task) + latest_task.duration;
            Task {
                name: latest_task.name.clone(),
                state: TaskState::End,
                duration,
                begin_time: latest_task.begin_time,
                ..Task::default()
            }
        } else {
            let duration = Task::calculate_duration(&latest_task);
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

fn get_latest_task_local(file: &mut fs::File) -> Task {
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap();

    if content.is_empty() {
        return Task::default();
    }

    let last_line = content.lines().last().unwrap();
    let task = serde_json::from_str::<Task>(last_line).unwrap();

    task
}

fn retrieve_user_key(file: &mut fs::File) -> String {
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap();
    let user_key = content.trim();

    user_key.to_string()
}

// const SERVICE_URL: &'static str = "https://imon-service.shuttleapp.rs";
const SERVICE_DOMAIN: &'static str = "http://localhost:8000";

// create an object to store service urls
struct Endpoints {
    auth: String,
    post_task_payload: String,
}

fn main() {
    let endpoints = Endpoints {
        auth: format!("{}{}", SERVICE_DOMAIN, "/v1/credentials"),
        post_task_payload: format!("{}{}", SERVICE_DOMAIN, "/v1/store"),
    };
    let request_client = reqwest::blocking::Client::new();

    let user_path = PathBuf::from("/tmp/imon-user.txt");
    let mut user_file = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(&user_path)
        .unwrap();
    // Format: $user_name:$id
    let current_user_key = retrieve_user_key(&mut user_file);
    let current_user_name = current_user_key.split(":").collect::<Vec<&str>>()[0];

    let path = PathBuf::from("/tmp/imon-tmp.txt");
    let mut file = fs::File::options()
        .read(true)
        .append(true)
        .create(true)
        .open(path)
        .unwrap();

    let latest_task = get_latest_task_local(&mut file);

    let cli = Cli::parse();

    if let Some(command) = &cli.command {
        match command {
            Commands::On { name } => {
                if current_user_key.is_empty() {
                    println!("Please register yourself first.");
                    return;
                }

                if latest_task.state == TaskState::Begin || latest_task.state == TaskState::Break {
                    println!(
                        "You are already working on `{}`. Please finish it first.",
                        latest_task.name
                    );
                    return;
                }

                let new_task = Task::generate_begin_task(name.as_ref().unwrap().to_string());

                println!("Sure, you are.");

                match request_client
                    .post(endpoints.post_task_payload)
                    .json(&serde_json::json!({
                        "user_name": current_user_key,
                        "task": new_task,
                    }))
                    .send()
                {
                    Ok(r) => {
                        match r.error_for_status() {
                            Ok(res) => {
                                println!("{:?}", res);
                                let json_r = res.json::<TaskResponse>().unwrap();
                                println!("{:?}", json_r);

                                if let Err(e) =
                                    writeln!(file, "{}", serde_json::to_string(&new_task).unwrap())
                                {
                                    eprintln!("Couldn't write to file: {}", e);
                                }
                            }
                            Err(e) => {
                                println!("eft {:?}", e);
                            }
                        };
                    }
                    Err(e) => {
                        println!("{:?}", e);
                    }
                };
            }
            Commands::Break => {
                if latest_task.state == TaskState::Break {
                    println!("You are already on break.");
                    return;
                } else if latest_task.state == TaskState::End {
                    println!("You are not working on anything.");
                    return;
                }

                let new_task = Task::generate_break_task(&latest_task);

                println!("Really?");

                match request_client
                    .post(endpoints.post_task_payload)
                    .json(&serde_json::json!({
                        "user_name": current_user_key,
                        "task": new_task,
                    }))
                    .send()
                {
                    Ok(r) => {
                        match r.error_for_status() {
                            Ok(res) => {
                                println!("{:?}", res);
                                let json_r = res.json::<TaskResponse>().unwrap();
                                println!("{:?}", json_r);

                                if let Err(e) =
                                    writeln!(file, "{}", serde_json::to_string(&new_task).unwrap())
                                {
                                    eprintln!("Couldn't write to file: {}", e);
                                }
                            }
                            Err(e) => {
                                println!("eft {:?}", e);
                            }
                        };
                    }
                    Err(e) => {
                        println!("{:?}", e);
                    }
                };
            }
            Commands::Back {} => {
                if latest_task.state == TaskState::Begin {
                    println!("You are already working on `{}`.", latest_task.name);
                    return;
                } else if latest_task.state == TaskState::End {
                    println!("You are not working on anything.");
                    return;
                }

                let new_task = Task::generate_back_task(&latest_task);

                println!("Ah, finally.");

                match request_client
                    .post(endpoints.post_task_payload)
                    .json(&serde_json::json!({
                        "user_name": current_user_key,
                        "task": new_task,
                    }))
                    .send()
                {
                    Ok(r) => {
                        match r.error_for_status() {
                            Ok(res) => {
                                println!("{:?}", res);
                                let json_r = res.json::<TaskResponse>().unwrap();
                                println!("{:?}", json_r);

                                if let Err(e) =
                                    writeln!(file, "{}", serde_json::to_string(&new_task).unwrap())
                                {
                                    eprintln!("Couldn't write to file: {}", e);
                                }
                            }
                            Err(e) => {
                                println!("eft {:?}", e);
                            }
                        };
                    }
                    Err(e) => {
                        println!("{:?}", e);
                    }
                };
            }
            Commands::Done {} => {
                if latest_task.state == TaskState::End {
                    println!("You are not working on anything.");
                    return;
                }

                let new_task = Task::generate_done_task(&latest_task);

                println!(
                    "You have worked on `{}` for {}.",
                    new_task.name, new_task.duration,
                );

                match request_client
                    .post(endpoints.post_task_payload)
                    .json(&serde_json::json!({
                        "user_name": current_user_key,
                        "task": new_task,
                    }))
                    .send()
                {
                    Ok(r) => {
                        println!("{:?}", r);
                        let json_r = r.json::<TaskResponse>().unwrap();

                        match json_r.status.as_str() {
                            "ok" => {
                                if let Err(e) =
                                    writeln!(file, "{}", serde_json::to_string(&new_task).unwrap())
                                {
                                    eprintln!("Couldn't write to file: {}", e);
                                }
                            }
                            "error" => match json_r.message {
                                Some(message) => {
                                    println!("{}", message);
                                }
                                None => {
                                    println!("Something went wrong.");
                                }
                            },
                            _ => {
                                println!("Something went wrong.");
                            }
                        }
                    }
                    Err(e) => {
                        println!("{:?}", e);
                    }
                };
            }
            Commands::Check {} => {
                println!("You are working on `{}`.", latest_task.name);
            }
            Commands::Auth { user_name } => {
                if !current_user_name.is_empty() {
                    println!("You are already registered as `{}`.", current_user_name);
                    println!("Please unregister first.");
                    return;
                }

                match request_client
                    .post(endpoints.auth)
                    .json(&serde_json::json!({
                        "user_name": user_name,
                    }))
                    .send()
                {
                    Ok(r) => {
                        let json_r = r.json::<AuthResponse>().unwrap();
                        println!("{:?}", json_r);

                        let mut user_file = fs::File::options()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(user_path)
                            .unwrap();

                        if let Err(e) = user_file.write_all(&json_r.data.user_key.into_bytes()) {
                            eprintln!("Couldn't write to file: {}", e);
                            return;
                        }
                    }
                    Err(e) => {
                        println!("{:?}", e);
                    }
                };

                println!("Drink water, {}.", user_name.as_ref().unwrap());
            }
        }
    } else {
        if current_user_name.is_empty() {
            println!("Please register yourself.");
        } else {
            println!(
                "{}. You are {}",
                current_user_name.to_uppercase(),
                current_user_name
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_latest_task_local() {
        let mut file = fs::File::options()
            .read(true)
            .append(true)
            .create(true)
            .open("/tmp/imon-tmp.txt")
            .unwrap();

        let parts_by_space = get_latest_task_local(&mut file);

        // assert_eq!(parts_by_space.len(), 0);
    }
}
