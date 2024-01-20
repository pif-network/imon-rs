use reqwest::{blocking::Client, Method};
use serde::Serialize;

pub fn make_request<T, B>(
    request_client: &Client,
    method: Method,
    url: &str,
    body: T,
) -> Result<B, String>
where
    T: Serialize,
    B: std::fmt::Debug + serde::de::DeserializeOwned,
{
    let resp = request_client
        .request(method, url)
        .json(&body)
        .send()
        .map_err(|e| format!("Error sending request: {}", e))?;

    let status = resp.status();

    if status.is_success() {
        let body = resp.json::<B>().unwrap();
        println!("{:?}", body);
        Ok(body)
    } else {
        Err(format!("Error: {:?}", status))
    }
}
