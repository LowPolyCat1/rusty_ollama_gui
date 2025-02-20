mod streaming_ollama;

use iced::advanced::graphics::image::image_rs::ImageFormat;
use iced::widget::{button, center, column, text, Column};
use iced::window::settings::PlatformSpecific;
use iced::window::Position;
use iced::{Center, Element, Right, Settings, Size, Subscription};
use uuid;

pub fn main() -> iced::Result {
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
        Some(ImageFormat::Png),
    )
    .unwrap();
    iced::window::Settings {
        size: Size::new(1080.0, 720.0),
        position: Position::Centered, // at some point change this to Position::SpecificWith for based on previous location
        min_size: Some(Size::new(300.0, 100.0)),
        max_size: None,
        visible: true,
        resizable: true,
        decorations: true,
        transparent: false,
        level: iced::window::Level::Normal,
        icon: Some(icon),
        platform_specific: PlatformSpecific::default(),
        exit_on_close_request: false,
    }
}

#[derive(Debug)]
struct OllamaGUI {
    responses: Vec<Ollama>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Add,
    Request(uuid::Uuid),
    RequestProgressed(
        (
            uuid::Uuid,
            Result<streaming_ollama::OllamaStreamProgress, streaming_ollama::Error>,
        ),
    ),
}

impl OllamaGUI {
    fn new() -> Self {
        Self {
            responses: vec![Ollama::new()],
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Add => {
                self.responses.push(Ollama::new());
            }
            Message::Request(id) => {
                if let Some(request) = self.responses.iter_mut().find(|ollama| ollama.uuid == id) {
                    request.start();
                }
            }
            Message::RequestProgressed((id, progress)) => {
                if let Some(request) = self.responses.iter_mut().find(|ollama| ollama.uuid == id) {
                    request.progress(progress);
                }
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(self.responses.iter().map(Ollama::subscription))
    }

    fn view(&self) -> Element<Message> {
        let responses = Column::with_children(self.responses.iter().map(Ollama::view))
            .push(
                button("Add another stream")
                    .on_press(Message::Add)
                    .padding(10),
            )
            .spacing(20)
            .align_x(Right);

        center(responses).padding(20).into()
    }
}

impl Default for OllamaGUI {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
enum OllamaStreamState {
    Idle,
    Streaming { output: String },
    Finished { output: String, context: Vec<u64> },
    Errored,
}

#[derive(Debug)]
struct Ollama {
    uuid: uuid::Uuid,
    state: OllamaStreamState,
}

impl Ollama {
    pub fn new() -> Self {
        Ollama {
            uuid: uuid::Uuid::new_v4(),
            state: OllamaStreamState::Idle,
        }
    }

    pub fn start(&mut self) {
        // Transition into a streaming state when starting
        match self.state {
            OllamaStreamState::Idle
            | OllamaStreamState::Finished { .. }
            | OllamaStreamState::Errored => {
                self.state = OllamaStreamState::Streaming {
                    output: String::new(),
                };
            }
            OllamaStreamState::Streaming { .. } => {}
        }
    }

    pub fn progress(
        &mut self,
        new_progress: Result<streaming_ollama::OllamaStreamProgress, streaming_ollama::Error>,
    ) {
        if let OllamaStreamState::Streaming { ref mut output } = self.state {
            match new_progress {
                Ok(streaming_ollama::OllamaStreamProgress::Streaming { token }) => {
                    output.push_str(&token);
                }
                Ok(streaming_ollama::OllamaStreamProgress::Finished { context }) => {
                    // When finished, preserve the final output and store the context.
                    let final_output = if let OllamaStreamState::Streaming { output } =
                        std::mem::replace(&mut self.state, OllamaStreamState::Idle)
                    {
                        output
                    } else {
                        String::new()
                    };
                    self.state = OllamaStreamState::Finished {
                        output: final_output,
                        context,
                    };
                }
                Err(_error) => {
                    self.state = OllamaStreamState::Errored;
                }
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match self.state {
            OllamaStreamState::Streaming { .. } => {
                // Replace the URL with your actual Ollama streaming endpoint.
                streaming_ollama::subscribe_to_stream(
                    self.uuid,
                    "http://localhost:11434/api/generate",
                )
                .map(Message::RequestProgressed)
            }
            _ => Subscription::none(),
        }
    }

    pub fn view(&self) -> Element<Message> {
        let control: Element<_> = match &self.state {
            OllamaStreamState::Idle => button("Start streaming from Ollama!")
                .on_press(Message::Request(self.uuid))
                .into(),
            OllamaStreamState::Streaming { output } => column!["Streaming output:", text(output)]
                .spacing(10)
                .align_x(Center)
                .into(),
            OllamaStreamState::Finished { output, context } => column![
                "Streaming finished!",
                "Final output:",
                text(output),
                "Context:",
                text(format!("{:?}", context)),
                button("Start again").on_press(Message::Request(self.uuid))
            ]
            .spacing(10)
            .align_x(Center)
            .into(),
            OllamaStreamState::Errored => column![
                "Something went wrong :(",
                button("Try again").on_press(Message::Request(self.uuid))
            ]
            .spacing(10)
            .align_x(Center)
            .into(),
        };

        Column::new()
            .spacing(10)
            .padding(10)
            .align_x(Center)
            .push(control)
            .into()
    }
}
