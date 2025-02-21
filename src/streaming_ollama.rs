use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream::try_channel;
use iced::Subscription;
use serde_json::json;
use std::hash::Hash;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum OllamaStreamProgress {
    Streaming { token: String },
    Finished { context: Vec<u64> },
}

#[derive(Debug, Clone)]
pub enum Error {
    RequestFailed(Arc<reqwest::Error>),
    NoContentLength,
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Error::RequestFailed(Arc::new(error))
    }
}

pub fn subscribe_to_stream<I: 'static + Hash + Copy + Send + Sync, T: ToString>(
    id: I,
    url: T,
    prompt: &str,
    model: &str,
    context: Option<Vec<u64>>,
) -> Subscription<(I, Result<OllamaStreamProgress, Error>)> {
    Subscription::run_with_id(
        id,
        fetch_and_stream_response(
            url.to_string(),
            prompt.to_string(),
            model.to_string(),
            context,
        )
        .map(move |progress| (id, progress)),
    )
}

fn fetch_and_stream_response(
    url: String,
    prompt: String,
    model: String,
    context: Option<Vec<u64>>,
) -> impl Stream<Item = Result<OllamaStreamProgress, Error>> {
    try_channel(1, move |mut output| async move {
        let client = reqwest::Client::new();
        let mut body = json!({
            "model": model,
            "prompt": prompt,
            "stream": true
        });

        if let Some(context) = context {
            body["context"] = json!(context);
        }

        let response = client.post(&url).json(&body).send().await?;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let chunk_str = String::from_utf8_lossy(&chunk).to_string();

            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&chunk_str) {
                if json_value
                    .get("done")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    let context = json_value
                        .get("context")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|x| x.as_u64()).collect::<Vec<u64>>())
                        .unwrap_or_else(Vec::new);
                    let _ = output
                        .send(OllamaStreamProgress::Finished { context })
                        .await;
                    break;
                } else {
                    let token = json_value
                        .get("response")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&chunk_str)
                        .to_string();
                    let _ = output.send(OllamaStreamProgress::Streaming { token }).await;
                }
            } else {
                let _ = output
                    .send(OllamaStreamProgress::Streaming { token: chunk_str })
                    .await;
            }
        }
        Ok(())
    })
}
