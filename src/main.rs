use iced::alignment::Horizontal;
use iced::futures::StreamExt;
use iced::widget::{button, center, column, container, row, scrollable, text, Column};
use iced::window::settings::PlatformSpecific;
use iced::window::Position;
use iced::{Alignment, Element, Length, Settings, Size, Subscription};
use serde::{Deserialize, Serialize};
use std::fs;
use tracing;
use tracing_subscriber;
use uuid;

mod streaming_ollama;
use streaming_ollama::{subscribe_to_stream, Error as OllamaError, OllamaStreamProgress};

pub fn main() -> iced::Result {
    // tracing_subscriber::fmt().init();
    iced::application("Ollama GUI", OllamaGUI::update, OllamaGUI::view)
        .subscription(OllamaGUI::subscription)
        .settings(settings())
        .window(windows_settings())
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

#[derive(Debug)]
struct OllamaGUI {
    chats: Vec<OllamaChat>,
    current_chat: uuid::Uuid,
}

#[derive(Debug, Clone)]
pub enum Message {
    NewChat,
    StartChat(uuid::Uuid),
    ChatProgress((uuid::Uuid, Result<OllamaStreamProgress, OllamaError>)),
    SelectChat(uuid::Uuid),
    PromptChanged(uuid::Uuid, String),
}

impl OllamaGUI {
    fn new() -> Self {
        let mut chats = Vec::new();

        // Load existing chats from ./chats directory
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

        // If no chats found, create initial chat
        let initial_chat = if chats.is_empty() {
            OllamaChat::new()
        } else {
            chats[0].clone()
        };

        let current_chat = initial_chat.uuid;

        if chats.is_empty() {
            chats.push(initial_chat);
        }

        Self {
            chats,
            current_chat,
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
            }
            Message::PromptChanged(id, new_prompt) => {
                if let Some(chat) = self.chats.iter_mut().find(|c| c.uuid == id) {
                    chat.input_prompt = new_prompt;
                }
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(self.chats.iter().map(|chat| chat.subscription()))
    }

    fn view(&self) -> Element<Message> {
        let sidebar = scrollable(
            column(self.chats.iter().map(|chat| {
                let is_current = chat.uuid == self.current_chat;
                chat.sidebar_view(is_current)
            }))
            .spacing(10),
        )
        .width(Length::Fixed(200.0))
        .height(Length::Fill);

        let current_chat = self
            .chats
            .iter()
            .find(|c| c.uuid == self.current_chat)
            .map(|chat| chat.main_view())
            .unwrap_or_else(|| column![].into());

        let main_content = container(current_chat)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .align_x(Horizontal::Center);

        row![
            sidebar,
            container(main_content)
                .width(Length::Fill)
                .height(Length::Fill)
        ]
        .spacing(20)
        .padding(10)
        .into()
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
    Streaming { output: String },
    Finished { output: String, context: Vec<u64> },
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

#[derive(Debug, Serialize, Deserialize)]
struct ChatEntry {
    prompt: String,
    response: String,
}

#[derive(Debug, Clone)]
struct OllamaChat {
    uuid: uuid::Uuid,
    display_name: String,
    state: ChatState,
    prompt: String,
    model: String,
    context: Option<Vec<u64>>,
    input_prompt: String,
}

impl OllamaChat {
    pub fn new() -> Self {
        let uuid = uuid::Uuid::new_v4();
        Self {
            uuid,
            display_name: format!("Chat-{}", uuid),
            state: ChatState::Idle,
            prompt: "prompt".to_string(),
            model: "phi4".to_string(),
            context: None,
            input_prompt: "prompt".to_string(),
        }
    }

    pub fn start(&mut self) {
        if let ChatState::Idle | ChatState::Finished { .. } | ChatState::Errored = self.state {
            // Update the actual prompt with the input value
            self.prompt = self.input_prompt.clone();
            self.state = ChatState::Streaming {
                output: String::new(),
            };
        }
    }

    pub fn from_history(history: ChatHistory) -> Result<Self, uuid::Error> {
        let uuid = uuid::Uuid::parse_str(&history.uuid)?;
        let last_entry = history.chat.last();

        Ok(Self {
            uuid,
            display_name: history.display_name,
            state: ChatState::Finished {
                output: last_entry.map(|e| e.response.clone()).unwrap_or_default(),
                context: history.context.clone(),
            },
            prompt: last_entry
                .map(|e| e.prompt.clone())
                .unwrap_or_else(|| "prompt".to_string()),
            model: history.model,
            context: Some(history.context),
            input_prompt: last_entry
                .map(|e| e.prompt.clone())
                .unwrap_or_else(|| "prompt".to_string()),
        })
    }

    pub fn progress(&mut self, progress: Result<OllamaStreamProgress, OllamaError>) {
        if let ChatState::Streaming { ref mut output } = self.state {
            match progress {
                Ok(OllamaStreamProgress::Streaming { token }) => {
                    output.push_str(&token);
                }
                Ok(OllamaStreamProgress::Finished { context }) => {
                    let final_output = std::mem::replace(output, String::new());
                    self.context = Some(context.clone());
                    self.state = ChatState::Finished {
                        output: final_output,
                        context,
                    };
                    self.save_chat_history();
                }
                Err(_) => {
                    self.state = ChatState::Errored;
                }
            }
        }
    }

    fn save_chat_history(&self) {
        if let ChatState::Finished { output, context } = &self.state {
            let chat_entry = ChatEntry {
                prompt: self.prompt.clone(),
                response: output.clone(),
            };

            let file_path = format!("./chats/{}.json", self.uuid);
            let _ = std::fs::create_dir_all("./chats");

            let mut chat_history = match std::fs::File::open(&file_path) {
                Ok(file) => serde_json::from_reader(file).unwrap_or_else(|_| ChatHistory {
                    display_name: self.display_name.clone(),
                    uuid: self.uuid.to_string(),
                    context: vec![],
                    model: self.model.clone(),
                    chat: vec![],
                }),
                Err(_) => ChatHistory {
                    display_name: self.display_name.clone(),
                    uuid: self.uuid.to_string(),
                    context: vec![],
                    model: self.model.clone(),
                    chat: vec![],
                },
            };

            chat_history.context = context.clone();
            chat_history.chat.push(chat_entry);

            if let Ok(file) = std::fs::File::create(&file_path) {
                let _ = serde_json::to_writer_pretty(file, &chat_history);
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if let ChatState::Streaming { .. } = self.state {
            subscribe_to_stream(
                self.uuid,
                "http://localhost:11434/api/generate",
                &self.prompt,
                &self.model,
            )
            .map(Message::ChatProgress)
        } else {
            Subscription::none()
        }
    }

    fn sidebar_view(&self, is_selected: bool) -> Element<Message> {
        let status_icon = match self.state {
            ChatState::Idle => text("●"),
            ChatState::Streaming { .. } => text("↻"),
            ChatState::Finished { .. } => text("✓"),
            ChatState::Errored => text("⚠"),
        };

        let content = row![status_icon, text(&self.display_name).size(14),]
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
        let controls = match &self.state {
            ChatState::Idle => column![
                button("Start Chat").on_press(Message::StartChat(self.uuid)),
                text(format!("Model: {}", &self.model))
            ],
            ChatState::Streaming { output } => column![
                // text("Streaming..."),
                text(output).width(500),
                button("Stop").on_press(Message::SelectChat(self.uuid)),
            ],
            ChatState::Finished { output, context } => column![
                // text("Completed:"),
                text(output),
                // text(format!("Context: {:?}", context)),
                button("Restart").on_press(Message::StartChat(self.uuid))
            ],
            ChatState::Errored => column![
                text("Error occurred"),
                button("Retry").on_press(Message::StartChat(self.uuid))
            ],
        };
        let prompt_input = iced::widget::text_input("Type your prompt...", &self.input_prompt)
            .on_input(move |text| Message::PromptChanged(self.uuid, text))
            .padding(10)
            .size(16);

        column![
            text(&self.display_name).size(24),
            controls.spacing(20),
            prompt_input
        ]
        .spacing(20)
        .align_x(Alignment::Start)
        .into()
    }
}
