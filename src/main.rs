use iced::alignment::Horizontal;
use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::window::settings::PlatformSpecific;
use iced::window::Position;
use iced::{Alignment, Element, Length, Settings, Size, Subscription};
use serde::{Deserialize, Serialize};
use std::fs;
use uuid::Uuid;

mod streaming_ollama;
use streaming_ollama::{subscribe_to_stream, Error as OllamaError, OllamaStreamProgress};

pub fn main() -> iced::Result {
    iced::application("Ollama GUI", OllamaGUI::update, OllamaGUI::view)
        .subscription(OllamaGUI::subscription)
        .settings(settings())
        .window(windows_settings())
        .theme(OllamaGUI::theme)
        .run()
}

fn settings() -> Settings {
    Settings::default()
}

fn windows_settings() -> iced::window::Settings {
    let icon = iced::window::icon::from_file_data(
        include_bytes!("../images/logo.png"),
        Some(iced::advanced::graphics::image::image_rs::ImageFormat::Png),
    )
    .unwrap();
    iced::window::Settings {
        size: Size::new(1080.0, 720.0),
        position: Position::Centered,
        min_size: Some(Size::new(300.0, 100.0)),
        max_size: None,
        visible: true,
        resizable: true,
        decorations: true,
        transparent: false,
        level: iced::window::Level::Normal,
        icon: Some(icon),
        platform_specific: PlatformSpecific::default(),
        exit_on_close_request: true,
    }
}

#[derive(Debug, Clone)]
pub enum AppState {
    Chat,
    Settings,
}
#[derive(Debug)]
struct OllamaGUI {
    chats: Vec<OllamaChat>,
    current_chat: Uuid,
    editing_chat: Option<Uuid>,
    state: AppState,
    default_url: String,
    theme: iced::Theme,
}

#[derive(Debug, Clone)]
pub enum Message {
    NewChat,
    StartChat(Uuid),
    ChatProgress((Uuid, Result<OllamaStreamProgress, OllamaError>)),
    SelectChat(Uuid),
    PromptChanged(Uuid, String),
    StartRenameChat(Uuid),
    FinishRenameChat(Uuid),
    CancelRenameChat(Uuid),
    UpdateTempName(Uuid, String),
    DeleteChat(Uuid),
    ChangeAppState(AppState),
    ChangeTheme(iced::Theme),
}

impl OllamaGUI {
    fn new() -> Self {
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
            default_url: "http://localhost:11434".to_string(),
            theme: iced::Theme::GruvboxDark,
        }
    }

    fn update(&mut self, message: Message) {
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
                let _ = std::fs::remove_file(format!("./chats/{}.json", uuid));
            }
            Message::ChangeAppState(app_state) => self.state = app_state,
            Message::ChangeTheme(theme) => self.theme = theme,
        }
    }

    fn theme(&self) -> iced::Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(self.chats.iter().map(|chat| chat.subscription()))
    }

    fn view(&self) -> Element<Message> {
        match self.state {
            AppState::Chat => {
                let sidebar_chats = scrollable(column(self.chats.iter().map(|chat| {
                    {
                        chat.sidebar_view(
                            chat.uuid == self.current_chat,
                            self.editing_chat == Some(chat.uuid),
                        )
                    }
                    // .collect::<Vec<_>>()
                })))
                .spacing(10)
                .width(Length::Fill)
                .height(Length::Fill);

                let left_sidebar = column![
                    row![
                        button("New Chat")
                            .on_press(Message::NewChat)
                            .width(Length::Shrink)
                            .padding([5, 10]),
                        button("Settings")
                            .on_press(Message::ChangeAppState(AppState::Settings))
                            .padding([5, 10])
                    ]
                    .padding([5, 10])
                    .width(Length::Shrink),
                    sidebar_chats
                ]
                .spacing(10)
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

                let right_sidebar = column![];

                row![left_sidebar, main_content, right_sidebar]
                    .spacing(20)
                    .padding(10)
                    .into()
            }
            AppState::Settings => column![row![
                button("Chats")
                    .on_press(Message::ChangeAppState(AppState::Chat))
                    .width(Length::Shrink)
                    .padding([5, 10]),
                button("Settings")
                    .on_press(Message::ChangeAppState(AppState::Settings))
                    .padding([5, 10]),
                iced::widget::pick_list(iced::Theme::ALL, Some(self.theme()), Message::ChangeTheme)
            ]
            .padding([5, 10])
            .width(Length::Shrink)]
            .spacing(10)
            .width(Length::FillPortion(1))
            .into(),
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
            display_name: format!("Chat-{}", uuid),
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

    pub fn progress(&mut self, progress: Result<OllamaStreamProgress, OllamaError>) {
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
        let _ = std::fs::create_dir_all("./chats");

        let chat_history = ChatHistory {
            display_name: self.display_name.clone(),
            uuid: self.uuid.to_string(),
            context: self.context.clone().unwrap_or_default(),
            model: self.model.clone(),
            chat: self.chat_entries.clone(),
        };

        if let Ok(file) = std::fs::File::create(&file_path) {
            let _ = serde_json::to_writer_pretty(file, &chat_history);
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if let ChatState::Streaming = self.state {
            subscribe_to_stream(
                self.uuid,
                "http://localhost:11434/api/generate",
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
                button("🗑").on_press(Message::DeleteChat(self.uuid)) // .style(iced::theme::Button::Destructive)
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
            container(content)
                // .style(iced::theme::Container::Box)
                .padding(5)
                .width(Length::Fill)
                .into()
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
                            text(format!("You: {}", entry.prompt)),
                            text(format!("AI: {}", entry.response)),
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

        let on_submit_message = {
            match self.state {
                ChatState::Streaming => None,
                _ => Some(Message::StartChat(self.uuid)),
            }
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
