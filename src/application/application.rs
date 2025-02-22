use iced::alignment::{Horizontal, Vertical};
use iced::border::Radius;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream::try_channel;
use iced::theme::Theme as IcedTheme;
use iced::widget::{button, column, container, progress_bar, row, scrollable, text, text_input};
use iced::{futures, Alignment, Border, Color, Element, Length, Subscription};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct AppSettings {
    theme: String,
    default_url: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: format!("{:?}", IcedTheme::KanagawaDragon),
            default_url: "http://localhost:11434".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AppState {
    Chat,
    Settings,
}

#[derive(Debug)]
pub struct OllamaGUI {
    chats: Vec<OllamaChat>,
    current_chat: Uuid,
    editing_chat: Option<Uuid>,
    state: AppState,
    default_url: String,
    theme: iced::Theme,
    download_model_input: String,
    // Support multiple downloads concurrently.
    download_progress: Vec<DownloadProgress>,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub id: Uuid,
    pub model: String,
    pub status: String,
    pub total: u64,
    pub completed: u64,
}

#[derive(Debug, Clone)]
pub enum Message {
    NewChat,
    StartChat(Uuid),
    ChatProgress((Uuid, Result<OllamaStreamProgress, Error>)),
    SelectChat(Uuid),
    PromptChanged(Uuid, String),
    StartRenameChat(Uuid),
    FinishRenameChat(Uuid),
    CancelRenameChat(Uuid),
    UpdateTempName(Uuid, String),
    DeleteChat(Uuid),
    ChangeAppState(AppState),
    ChangeTheme(iced::Theme),
    ChangeDefaultUrl(String),
    DownloadModelInputChanged(String),
    StartDownloadModel,
    DownloadProgress(Uuid, Result<DownloadProgressUpdate, Error>),
    // New message variant for canceling a download.
    CancelDownload(Uuid),
}

#[derive(Debug, Clone)]
pub enum DownloadProgressUpdate {
    Progress {
        status: String,
        total: u64,
        completed: u64,
    },
    Finished,
}

#[derive(Debug, Clone)]
pub enum OllamaStreamProgress {
    Streaming { token: String },
    Finished { context: Vec<u64> },
}

#[derive(Debug, Clone)]
pub enum Error {
    RequestFailed(Arc<reqwest::Error>),
    ParseError(Arc<serde_json::Error>),
    ChannelError(Arc<futures::channel::mpsc::SendError>),
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Error::RequestFailed(Arc::new(error))
    }
}

impl From<futures::channel::mpsc::SendError> for Error {
    fn from(e: futures::channel::mpsc::SendError) -> Self {
        Error::ChannelError(Arc::new(e))
    }
}

impl OllamaGUI {
    pub fn load_settings() -> AppSettings {
        let path = PathBuf::from("./settings/settings.json");
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
                Err(_) => AppSettings::default(),
            }
        } else {
            let _ = fs::create_dir_all("./settings");
            AppSettings::default()
        }
    }

    pub fn save_settings(&self) {
        let path = PathBuf::from("./settings/settings.json");
        let settings = AppSettings {
            theme: format!("{:?}", self.theme),
            default_url: self.default_url.clone(),
        };
        let _ = fs::write(path, serde_json::to_string_pretty(&settings).unwrap());
    }

    pub fn new() -> Self {
        let settings = Self::load_settings();
        let mut theme = IcedTheme::GruvboxDark;

        for available in IcedTheme::ALL {
            if format!("{:?}", available) == settings.theme {
                theme = available.clone();
                break;
            }
        }

        let mut chats = Vec::new();

        if let Ok(entries) = fs::read_dir("./chats") {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    if let Ok(file) = fs::File::open(&path) {
                        match serde_json::from_reader::<_, ChatHistory>(file) {
                            Ok(history) => {
                                if let Ok(chat) = OllamaChat::from_history(history) {
                                    chats.push(chat);
                                }
                            }
                            Err(e) => eprintln!("Error parsing chat file: {}", e),
                        }
                    }
                }
            }
        }

        let initial_chat = if chats.is_empty() {
            OllamaChat::new()
        } else {
            chats[0].clone()
        };

        Self {
            current_chat: initial_chat.uuid,
            chats: if chats.is_empty() {
                vec![initial_chat]
            } else {
                chats
            },
            editing_chat: None,
            state: AppState::Chat,
            default_url: settings.default_url,
            theme,
            download_model_input: String::new(),
            download_progress: Vec::new(),
        }
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::NewChat => {
                let new_chat = OllamaChat::new();
                self.current_chat = new_chat.uuid;
                self.chats.push(new_chat);
            }
            Message::StartChat(id) => {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.uuid == id) {
                    chat.start();
                }
            }
            Message::ChatProgress((id, progress)) => {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.uuid == id) {
                    chat.progress(progress);
                }
            }
            Message::SelectChat(id) => {
                self.current_chat = id;
                self.editing_chat = None;
            }
            Message::PromptChanged(id, new_prompt) => {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.uuid == id) {
                    chat.input_prompt = new_prompt;
                }
            }
            Message::StartRenameChat(uuid) => {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.uuid == uuid) {
                    chat.start_rename();
                    self.editing_chat = Some(uuid);
                }
            }
            Message::UpdateTempName(uuid, name) => {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.uuid == uuid) {
                    chat.update_temp_name(name);
                }
            }
            Message::FinishRenameChat(uuid) => {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.uuid == uuid) {
                    chat.finish_rename();
                    self.editing_chat = None;
                }
            }
            Message::CancelRenameChat(uuid) => {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.uuid == uuid) {
                    chat.cancel_rename();
                    self.editing_chat = None;
                }
            }
            Message::DeleteChat(uuid) => {
                self.chats.retain(|c| c.uuid != uuid);
                if self.current_chat == uuid {
                    self.current_chat = self.chats.first().map(|c| c.uuid).unwrap_or(Uuid::nil());
                }
                let _ = fs::remove_file(format!("./chats/{}.json", uuid));
            }
            Message::ChangeAppState(app_state) => self.state = app_state,
            Message::ChangeTheme(theme) => {
                self.theme = theme;
                self.save_settings();
            }
            Message::ChangeDefaultUrl(url) => {
                self.default_url = url;
                self.save_settings();
            }
            Message::DownloadModelInputChanged(input) => {
                self.download_model_input = input;
            }
            Message::StartDownloadModel => {
                if !self.download_model_input.is_empty() {
                    self.download_progress.push(DownloadProgress {
                        id: Uuid::new_v4(),
                        model: self.download_model_input.clone(),
                        status: "Starting download...".into(),
                        total: 0,
                        completed: 0,
                    });
                    self.download_model_input.clear();
                }
            }
            Message::DownloadProgress(id, result) => match result {
                Ok(DownloadProgressUpdate::Progress {
                    status,
                    total,
                    completed,
                }) => {
                    if let Some(dl) = self.download_progress.iter_mut().find(|d| d.id == id) {
                        dl.status = status;
                        dl.total = total;
                        dl.completed = completed;
                    }
                }
                Ok(DownloadProgressUpdate::Finished) => {
                    self.download_progress.retain(|dl| dl.id != id);
                }
                Err(e) => {
                    if let Some(dl) = self.download_progress.iter_mut().find(|d| d.id == id) {
                        dl.status = format!("Error: {:?}", e);
                    }
                }
            },
            Message::CancelDownload(id) => {
                // Remove the download entry when the user cancels it.
                self.download_progress.retain(|dl| dl.id != id);
            }
        }
    }

    pub fn theme(&self) -> iced::Theme {
        self.theme.clone()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let chat_subs = self
            .chats
            .iter()
            .map(|chat| chat.subscription(self.default_url.clone()));

        let download_subs = self.download_progress.iter().map(|dl| {
            subscribe_to_download(
                dl.id,
                format!("{}/api/pull", self.default_url),
                dl.model.clone(),
            )
            .map(|(id, result)| Message::DownloadProgress(id, result))
        });

        Subscription::batch(chat_subs.chain(download_subs))
    }

    pub fn view(&self) -> Element<Message> {
        let top_nav = row![
            button("Chats")
                .on_press(Message::ChangeAppState(AppState::Chat))
                .padding([5, 10])
                .width(Length::Shrink),
            button("Settings")
                .on_press(Message::ChangeAppState(AppState::Settings))
                .padding([5, 10])
                .width(Length::Shrink),
        ]
        .spacing(5)
        .padding([0, 5])
        .align_y(Alignment::Start);

        match self.state {
            AppState::Chat => {
                let sidebar_chats = scrollable(
                    column(self.chats.iter().map(|chat| {
                        chat.sidebar_view(
                            chat.uuid == self.current_chat,
                            self.editing_chat == Some(chat.uuid),
                        )
                    }))
                    .spacing(5),
                )
                .spacing(5)
                .width(Length::Fill)
                .height(Length::Fill);

                let left_sidebar = column![
                    row![button("New Chat")
                        .on_press(Message::NewChat)
                        .padding([5, 10])]
                    .padding([5, 0]),
                    sidebar_chats
                ]
                .spacing(5)
                .width(Length::FillPortion(1));

                let current_chat = self
                    .chats
                    .iter()
                    .find(|c| c.uuid == self.current_chat)
                    .map(|chat| chat.main_view())
                    .unwrap_or_else(|| column!().into());

                let main_content = container(current_chat)
                    .width(Length::FillPortion(4))
                    .height(Length::Fill)
                    .align_x(Horizontal::Center);

                column![
                    top_nav,
                    row![left_sidebar, main_content]
                        .spacing(10)
                        .padding(5)
                        .height(Length::Fill)
                ]
                .into()
            }
            AppState::Settings => {
                let downloads_view = if self.download_progress.is_empty() {
                    column!().into()
                } else {
                    column(
                        self.download_progress
                            .iter()
                            .map(|dl| -> Element<Message> {
                                let progress = if dl.total > 0 {
                                    dl.completed as f32 / dl.total as f32
                                } else {
                                    0.0
                                };
                                let percentage = if dl.total > 0 {
                                    (progress * 100.0) as u32
                                } else {
                                    0
                                };
                                Into::<Element<Message>>::into(
                                    row![
                                        Into::<Element<Message>>::into(
                                            column![
                                                text(format!(
                                                    "Downloading {}: {}%",
                                                    dl.model, percentage
                                                ))
                                                .size(16),
                                                progress_bar::<iced::Theme>(0.0..=1.0, progress)
                                                    .width(Length::Fixed(300.0)),
                                                text(&dl.status)
                                            ]
                                            .padding([5, 0])
                                            .spacing(5)
                                        ),
                                        button("Cancel")
                                            .on_press(Message::CancelDownload(dl.id))
                                            .padding([5, 10])
                                    ]
                                    .align_y(Vertical::Center)
                                    .spacing(10),
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                };

                column![
                    top_nav,
                    column![
                        text("Theme").size(16),
                        iced::widget::pick_list(
                            iced::Theme::ALL,
                            Some(self.theme()),
                            Message::ChangeTheme
                        )
                        .padding([5, 10])
                        .width(Length::Shrink),
                        text("Ollama URL").size(16),
                        text_input("http://localhost:11434", &self.default_url)
                            .on_input(Message::ChangeDefaultUrl)
                            .padding(5)
                            .width(Length::Fixed(300.0)),
                        text("Download Model").size(16),
                        row![
                            text_input("Model name", &self.download_model_input)
                                .on_input(Message::DownloadModelInputChanged)
                                .padding(5)
                                .width(Length::Fixed(300.0))
                                .on_submit_maybe(if self.download_model_input.is_empty() {
                                    None
                                } else {
                                    Some(Message::StartDownloadModel)
                                }),
                            button("Download")
                                .on_press_maybe(if self.download_model_input.is_empty() {
                                    None
                                } else {
                                    Some(Message::StartDownloadModel)
                                })
                                .padding([5, 10])
                        ],
                        downloads_view
                    ]
                    .spacing(10)
                    .padding(10)
                    .align_x(Alignment::Start)
                ]
                .into()
            }
        }
    }
}

impl Default for OllamaGUI {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
enum ChatState {
    Idle,
    Streaming,
    Finished,
    Errored,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatHistory {
    display_name: String,
    uuid: String,
    context: Vec<u64>,
    model: String,
    chat: Vec<ChatEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatEntry {
    prompt: String,
    response: String,
}

#[derive(Debug, Clone)]
struct OllamaChat {
    uuid: Uuid,
    display_name: String,
    editing_name: Option<String>,
    state: ChatState,
    input_prompt: String,
    model: String,
    context: Option<Vec<u64>>,
    chat_entries: Vec<ChatEntry>,
}

impl OllamaChat {
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        Self {
            uuid,
            display_name: "New Unnamed Chat".to_string(),
            editing_name: None,
            state: ChatState::Idle,
            input_prompt: String::new(),
            model: "phi4".to_string(),
            context: None,
            chat_entries: Vec::new(),
        }
    }

    pub fn from_history(history: ChatHistory) -> Result<Self, uuid::Error> {
        Ok(Self {
            uuid: Uuid::parse_str(&history.uuid)?,
            display_name: history.display_name,
            editing_name: None,
            state: ChatState::Finished,
            input_prompt: String::new(),
            model: history.model,
            context: Some(history.context),
            chat_entries: history.chat,
        })
    }

    pub fn start_rename(&mut self) {
        self.editing_name = Some(self.display_name.clone());
    }

    pub fn update_temp_name(&mut self, name: String) {
        if let Some(editing_name) = &mut self.editing_name {
            *editing_name = name;
        }
    }

    pub fn finish_rename(&mut self) {
        if let Some(name) = self.editing_name.take() {
            self.display_name = name.trim().to_string();
            self.save_chat_history();
        }
    }

    pub fn cancel_rename(&mut self) {
        self.editing_name = None;
    }

    pub fn start(&mut self) {
        if matches!(
            self.state,
            ChatState::Idle | ChatState::Finished | ChatState::Errored
        ) {
            self.chat_entries.push(ChatEntry {
                prompt: self.input_prompt.clone(),
                response: String::new(),
            });
            self.state = ChatState::Streaming;
            self.input_prompt.clear();
        }
    }

    pub fn progress(&mut self, progress: Result<OllamaStreamProgress, Error>) {
        if let ChatState::Streaming = self.state {
            match progress {
                Ok(OllamaStreamProgress::Streaming { token }) => {
                    if let Some(last_entry) = self.chat_entries.last_mut() {
                        last_entry.response.push_str(&token);
                    }
                }
                Ok(OllamaStreamProgress::Finished { context }) => {
                    self.context = Some(context);
                    self.state = ChatState::Finished;
                    self.save_chat_history();
                }
                Err(_) => {
                    self.state = ChatState::Errored;
                }
            }
        }
    }

    fn save_chat_history(&self) {
        let file_path = format!("./chats/{}.json", self.uuid);
        let _ = fs::create_dir_all("./chats");

        let chat_history = ChatHistory {
            display_name: self.display_name.clone(),
            uuid: self.uuid.to_string(),
            context: self.context.clone().unwrap_or_default(),
            model: self.model.clone(),
            chat: self.chat_entries.clone(),
        };

        if let Ok(file) = fs::File::create(&file_path) {
            let _ = serde_json::to_writer_pretty(file, &chat_history);
        }
    }

    pub fn subscription(&self, base_url: String) -> Subscription<Message> {
        if let ChatState::Streaming = self.state {
            let api_url = format!("{}/api/generate", base_url);
            subscribe_to_stream(
                self.uuid,
                api_url,
                &self.chat_entries.last().unwrap().prompt,
                &self.model,
                self.context.clone(),
            )
            .map(Message::ChatProgress)
        } else {
            Subscription::none()
        }
    }

    fn sidebar_view(&self, is_selected: bool, is_editing: bool) -> Element<Message> {
        let current_name = self.editing_name.as_deref().unwrap_or(&self.display_name);

        let controls = if is_editing {
            row![
                text_input("Chat Name", current_name)
                    .on_input(|s| Message::UpdateTempName(self.uuid, s))
                    .on_submit(Message::FinishRenameChat(self.uuid))
                    .padding(5)
                    .width(Length::Fill),
                row![
                    button("✓").on_press(Message::FinishRenameChat(self.uuid)),
                    button("✖").on_press(Message::CancelRenameChat(self.uuid))
                ]
                .spacing(5)
            ]
            .spacing(10)
        } else {
            row![
                text(current_name).width(Length::Fill),
                button("✎").on_press(Message::StartRenameChat(self.uuid)),
                button("🗑").on_press(Message::DeleteChat(self.uuid))
            ]
            .spacing(5)
        };

        let status_icon = match self.state {
            ChatState::Idle => text("●"),
            ChatState::Streaming => text("↻"),
            ChatState::Finished => text("✓"),
            ChatState::Errored => text("⚠"),
        };

        let content = row![status_icon, controls]
            .spacing(10)
            .align_y(Alignment::Center);

        if is_selected {
            container(content).padding(5).width(Length::Fill).into()
        } else {
            button(content)
                .on_press(Message::SelectChat(self.uuid))
                .padding(5)
                .width(Length::Fill)
                .into()
        }
    }

    fn main_view(&self) -> Element<Message> {
        let chat_log = scrollable(
            column(
                self.chat_entries
                    .iter()
                    .map(|entry| {
                        column![
                            text(format!("Prompt: {}", entry.prompt)),
                            // iced::widget::TextInput::new(
                            //     "",
                            //     &format!("{}: {}", self.model, entry.response)
                            // )
                            // .width(Length::Fill)
                            // .style(borderless_input_style()),
                            text(format!("{}: {}", self.model, entry.response)).width(Length::Fill)
                        ]
                        .spacing(5)
                        .padding(10)
                        .into()
                    })
                    .collect::<Vec<_>>(),
            )
            .spacing(10),
        )
        .height(Length::Fill);

        let on_submit_message = match self.state {
            ChatState::Streaming => None,
            _ => Some(Message::StartChat(self.uuid)),
        };

        let input_row = row![
            text_input("Type your prompt...", &self.input_prompt)
                .on_input(|s| Message::PromptChanged(self.uuid, s))
                .on_submit_maybe(on_submit_message)
                .padding(10)
                .width(Length::Fill),
            match self.state {
                ChatState::Idle | ChatState::Finished =>
                    button("Send").on_press(Message::StartChat(self.uuid)),
                ChatState::Streaming => button("Stop").on_press(Message::SelectChat(self.uuid)),
                ChatState::Errored => button("Retry").on_press(Message::StartChat(self.uuid)),
            }
        ]
        .spacing(10);

        column![text(&self.display_name).size(24), chat_log, input_row]
            .spacing(20)
            .padding(20)
            .height(Length::Fill)
            .into()
    }
}

fn borderless_input_style(
) -> impl Fn(&iced::Theme, iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    |theme, _status| iced::widget::text_input::Style {
        background: iced::Background::Color(Color::TRANSPARENT),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::new(0),
        },
        icon: Color::TRANSPARENT,
        placeholder: Color::TRANSPARENT,
        value: theme.palette().text,
        selection: Color::from_rgba8(0, 120, 212, 0.3),
    }
}

fn subscribe_to_stream<I: 'static + Hash + Copy + Send + Sync, T: ToString>(
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

fn subscribe_to_download<I: 'static + Hash + Send + Sync + Clone>(
    id: I,
    url: String,
    model: String,
) -> Subscription<(I, Result<DownloadProgressUpdate, Error>)> {
    Subscription::run_with_id(
        id.clone(),
        iced::futures::stream::unfold(
            (id, url, model, None),
            move |(id, url, model, client)| async move {
                let client = client.unwrap_or_else(|| reqwest::Client::new());
                let body = json!({ "model": model, "stream": true });

                match client.post(&url).json(&body).send().await {
                    Ok(response) => {
                        let mut stream = response.bytes_stream();
                        while let Some(chunk) = stream.next().await {
                            match chunk {
                                Ok(bytes) => {
                                    let chunk_str = String::from_utf8_lossy(&bytes);
                                    for line in chunk_str.lines() {
                                        match serde_json::from_str::<serde_json::Value>(line) {
                                            Ok(json) => {
                                                if let Some(status) =
                                                    json.get("status").and_then(|v| v.as_str())
                                                {
                                                    let total = json
                                                        .get("total")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(0);
                                                    let completed = json
                                                        .get("completed")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(0);

                                                    return Some((
                                                        (
                                                            id.clone(),
                                                            Ok(DownloadProgressUpdate::Progress {
                                                                status: status.to_string(),
                                                                total,
                                                                completed,
                                                            }),
                                                        ),
                                                        (id, url, model, Some(client)),
                                                    ));
                                                }
                                            }
                                            Err(e) => {
                                                return Some((
                                                    (
                                                        id.clone(),
                                                        Err(Error::ParseError(Arc::new(e))),
                                                    ),
                                                    (id, url, model, Some(client)),
                                                ))
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    return Some((
                                        (id.clone(), Err(Error::RequestFailed(Arc::new(e)))),
                                        (id, url, model, Some(client)),
                                    ))
                                }
                            }
                        }
                        None
                    }
                    Err(e) => Some((
                        (id.clone(), Err(Error::RequestFailed(Arc::new(e)))),
                        (id, url, model, Some(client)),
                    )),
                }
            },
        ),
    )
}
