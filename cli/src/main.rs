use std::path::PathBuf;
use std::{
    fs,
    io::{Read, Write},
};

use clap::{Parser, Subcommand};
use libs::payload::{RegisterRecordPayload, StoreTaskPayload};
use serde::{Deserialize, Serialize};

use libs::record::{Task, TaskState};

use crate::util::make_request;

pub mod util;

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

#[derive(Serialize, Deserialize, Debug)]
struct NewTaskResponse {
    status: String,
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
    #[command(subcommand)]
    Auth(AuthCommand),
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Register yourself.
    New { user_name: String },
    /// Login with `user_key`
    #[command(name = "login")]
    LogIn { user_key: String },
}

fn get_latest_task_local(file: &mut fs::File) -> Task {
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap();

    if content.is_empty() {
        return Task::placeholder("fresh", TaskState::Idle);
    }

    let last_line = content.lines().last().unwrap();
    serde_json::from_str::<Task>(last_line).unwrap()
}

fn retrieve_user_key(file: &mut fs::File) -> String {
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap();
    let user_key = content.trim();

    user_key.to_string()
}

// const SERVICE_URL: &'static str = "https://imon-service.shuttleapp.rs";
const SERVICE_DOMAIN: &str = "http://localhost:8000";

struct Endpoints {
    auth: String,
    post_task_payload: String,
    get_task_log: String,
}

fn main() {
    let endpoints = Endpoints {
        auth: format!("{}{}", SERVICE_DOMAIN, "/v1/record/new"),
        post_task_payload: format!("{}{}", SERVICE_DOMAIN, "/v1/task/new"),
        get_task_log: format!("{}{}", SERVICE_DOMAIN, "/v1/task-log"),
    };
    let request_client = reqwest::blocking::Client::new();

    let user_path = PathBuf::from("/tmp/imon-user.txt");
    let mut user_file = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(&user_path)
        .unwrap();
    // Format: $role:$user_name:$id
    let current_user_key = retrieve_user_key(&mut user_file);
    let current_user_name = current_user_key.split(':').nth(1).unwrap_or("");

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

                if latest_task.state == TaskState::Begin
                    || latest_task.state == TaskState::Break
                    || latest_task.state == TaskState::Back
                {
                    println!(
                        "You are already working on `{}`. Please finish it first.",
                        latest_task.name
                    );
                    return;
                }

                let new_task = Task::generate_begin_task(name.as_ref().unwrap().to_string());

                println!("Sure, you are.");

                let payload = StoreTaskPayload {
                    key: current_user_key.clone(),
                    task: new_task.clone(),
                };
                if let Err(e) = make_request::<_, NewTaskResponse>(
                    &request_client,
                    reqwest::Method::POST,
                    &endpoints.post_task_payload,
                    payload,
                ) {
                    eprintln!("Failed to post to upstream: {}", e);
                    return;
                }

                if let Err(e) = writeln!(file, "{}", serde_json::to_string(&new_task).unwrap()) {
                    eprintln!("Couldn't write to file: {}", e);
                }
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

                let payload = StoreTaskPayload {
                    key: current_user_key.clone(),
                    task: new_task.clone(),
                };
                if let Err(e) = make_request::<_, NewTaskResponse>(
                    &request_client,
                    reqwest::Method::POST,
                    &endpoints.post_task_payload,
                    payload,
                ) {
                    eprintln!("Failed to post to upstream: {}", e);
                    return;
                }

                if let Err(e) = writeln!(file, "{}", serde_json::to_string(&new_task).unwrap()) {
                    eprintln!("Couldn't write to file: {}", e);
                }
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

                let payload = StoreTaskPayload {
                    key: current_user_key.clone(),
                    task: new_task.clone(),
                };
                if let Err(e) = make_request::<_, NewTaskResponse>(
                    &request_client,
                    reqwest::Method::POST,
                    &endpoints.post_task_payload,
                    payload,
                ) {
                    eprintln!("Failed to post to upstream: {}", e);
                    return;
                }

                if let Err(e) = writeln!(file, "{}", serde_json::to_string(&new_task).unwrap()) {
                    eprintln!("Couldn't write to file: {}", e);
                }
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

                let payload = StoreTaskPayload {
                    key: current_user_key.clone(),
                    task: new_task.clone(),
                };
                if let Err(e) = make_request::<_, NewTaskResponse>(
                    &request_client,
                    reqwest::Method::POST,
                    &endpoints.post_task_payload,
                    payload,
                ) {
                    eprintln!("Failed to post to upstream: {}", e);
                    return;
                }

                if let Err(e) = writeln!(file, "{}", serde_json::to_string(&new_task).unwrap()) {
                    eprintln!("Couldn't write to file: {}", e);
                }
            }
            Commands::Check {} => {
                println!("You are working on `{}`.", latest_task.name);
            }
            Commands::Auth { 0: auth_command } => match auth_command {
                AuthCommand::New { user_name } => {
                    if !current_user_name.is_empty() {
                        println!("You are already registered as `{}`.", current_user_name);
                        println!("Please unregister first.");
                        return;
                    }

                    let payload = RegisterRecordPayload {
                        user_name: user_name.clone(),
                    };

                    match make_request::<_, AuthResponse>(
                        &request_client,
                        reqwest::Method::POST,
                        &endpoints.auth,
                        payload,
                    ) {
                        Ok(resp_body) => {
                            let mut user_file = fs::File::options()
                                .write(true)
                                .create(true)
                                .truncate(true)
                                .open(user_path)
                                .unwrap();

                            if let Err(e) =
                                user_file.write_all(&resp_body.data.user_key.into_bytes())
                            {
                                eprintln!("Couldn't write to file: {}", e);
                                return;
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to post to upstream: {}", e);
                            return;
                        }
                    }

                    println!("Drink water, {}.", user_name);
                }
                AuthCommand::LogIn { user_key } => {
                    if !current_user_name.is_empty() {
                        println!("You are already registered as `{}`.", current_user_name);
                        println!("Please unregister first.");
                        return;
                    }

                    match request_client
                        .post(endpoints.get_task_log)
                        .json(&serde_json::json!({
                            "key": user_key,
                        }))
                        .send()
                    {
                        Ok(r) => {
                            match r.error_for_status() {
                                Ok(res) => {
                                    let json_r = res.json::<TaskResponse>().unwrap();
                                    println!("{:?}", json_r);

                                    let mut user_file = fs::File::options()
                                        .write(true)
                                        .create(true)
                                        .truncate(true)
                                        .open(user_path)
                                        .unwrap();

                                    if let Err(e) = user_file.write_all(user_key.as_bytes()) {
                                        eprintln!("Couldn't write to file: {}", e);
                                        return;
                                    }
                                }
                                Err(e) => {
                                    if e.status().unwrap().is_client_error() {
                                        println!("User not found.");
                                    }
                                }
                            };
                        }
                        Err(e) => {
                            println!("{:?}", e);
                        }
                    };

                    println!("Drink water, {}.", user_key);
                }
            },
        }
    } else if current_user_name.is_empty() {
        println!("Please register yourself.");
    } else {
        println!(
            "{}. You are {}.",
            current_user_name.to_uppercase(),
            current_user_name
        );
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

        let _parts_by_space = get_latest_task_local(&mut file);
    }
}
