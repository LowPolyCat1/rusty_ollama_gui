#![windows_subsystem = "windows"]

mod application;
use application::application::*;
use application::iced_settings::iced_settings::*;

pub fn main() -> iced::Result {
    iced::application("Ollama GUI", OllamaGUI::update, OllamaGUI::view)
        .subscription(OllamaGUI::subscription)
        .settings(settings())
        .window(windows_settings())
        .theme(OllamaGUI::theme)
        .run()
}
