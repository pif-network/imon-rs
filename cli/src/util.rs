use reqwest::{blocking::Client, Method};
use serde::Serialize;

use crate::TaskResponse;

pub fn make_request<T>(
    request_client: &Client,
    method: Method,
    url: &str,
    body: T,
) -> Result<(), String>
where
    T: Serialize,
{
    let resp = request_client
        .request(method, url)
        .json(&body)
        .send()
        .map_err(|e| format!("Error sending request: {}", e))?;

    let status = resp.status();
    let body = resp.json::<TaskResponse>().unwrap();
    println!("{:?}", body);

    if status.is_success() {
        Ok(())
    } else {
        Err(format!("Error: {:?}", body.message))
    }
}
