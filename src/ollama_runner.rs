use futures::StreamExt;
use gtk4::{ScrolledWindow, TextBuffer, prelude::TextBufferExt};
use ollama_rs::{
    Ollama,
    generation::chat::{ChatMessage, request::ChatMessageRequest},
};
use serde::Deserialize;
use std::{sync::Arc, sync::Mutex};

pub async fn render(
    ollama: Ollama,
    buffer: TextBuffer,
    context_clone: Arc<Mutex<Vec<ChatMessage>>>,
    model: String,
    request: ChatMessageRequest,
) {
    match ollama
        .send_chat_messages_with_history_stream(Arc::clone(&context_clone), request)
        .await
    {
        Ok(mut stream) => {
            let mut has_generated = false;
            while let Some(chunk) = stream.next().await {
                let content = chunk.unwrap();

                if !content.message.tool_calls.is_empty() {
                    for tool in content.message.tool_calls.clone() {
                        buffer.set_text(&("Executing ".to_owned() + &tool.function.name));

                        if tool.function.name == "get_weather" {
                            let args = tool.function.arguments;
                            let location = args["location"].as_str().unwrap_or("Tokyo").to_string();

                            let api_key = "df8e10a50aae988bb75fca883e066bc5";
                            #[derive(Deserialize, Debug)]
                            struct Main {
                                temp: f32,
                                humidity: u32,
                            }

                            #[derive(Deserialize, Debug)]
                            struct WeatherCondition {
                                description: String,
                            }

                            #[derive(Deserialize, Debug)]
                            struct WeatherResponse {
                                main: Main,
                                weather: Vec<WeatherCondition>,
                                name: String,
                            }

                            let tool_message;

                            async fn fetch_weather(
                                client: &reqwest::Client,
                                location: &str,
                                api_key: &str,
                            ) -> Result<WeatherResponse, reqwest::Error>
                            {
                                client
                                    .get("https://api.openweathermap.org/data/2.5/weather")
                                    .query(&[
                                        ("q", location),
                                        ("units", "imperial"),
                                        ("appid", api_key),
                                    ])
                                    .send()
                                    .await?
                                    .error_for_status()? // turns 401, 404, etc. into an Err
                                    .json::<WeatherResponse>()
                                    .await
                            }

                            match fetch_weather(&reqwest::Client::new(), &location, api_key).await {
                                Ok(res) => {
                                    tool_message = ChatMessage::new(
                                        ollama_rs::generation::chat::MessageRole::Tool,
                                        res.main.temp.to_string()
                                            + "°F and "
                                            + &res.main.humidity.to_string()
                                            + "% humidity "
                                            + " with "
                                            + &res
                                                .weather
                                                .iter()
                                                .map(|x| x.description.clone())
                                                .collect::<String>()
                                                .to_owned(),
                                    )
                                }

                                Err(e) => {
                                    tool_message = ChatMessage::new(
                                        ollama_rs::generation::chat::MessageRole::Tool,
                                        "Error fetching: ".to_string().to_owned() + &e.to_string(),
                                    )
                                }
                            }

                            {
                                let mut history = context_clone.lock().unwrap();
                                history.push(content.message.clone());
                                history.push(tool_message);
                            }
                        }

                        buffer.set_text("Thinking...");
                    }

                    if let Ok(mut final_stream) = ollama
                        .send_chat_messages_with_history_stream(
                            Arc::clone(&context_clone),
                            ChatMessageRequest::new(model.clone(), vec![]),
                        )
                        .await
                    {
                        let mut has_generated = false;
                        while let Some(chunk) = final_stream.next().await {
                            if let Ok(content) = chunk {
                                if !has_generated && content.message.content != "" {
                                    buffer.set_text("");
                                    has_generated = true;
                                }
                                if content.final_data.is_none() {
                                    buffer.insert(&mut buffer.end_iter(), &content.message.content);
                                }
                            }
                        }
                    }
                } else {
                    if !has_generated && content.message.content != "" {
                        buffer.set_text("");
                        has_generated = true;
                    }
                    if !content.final_data.is_some() {
                        buffer.insert(&mut buffer.end_iter(), &content.message.content);
                    }
                }
            }
        }
        Err(e) => {
            buffer.set_text(&format!("[error: {e}]"));
        }
    }
}
