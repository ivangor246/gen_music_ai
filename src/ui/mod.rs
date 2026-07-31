mod density_canvas;
mod message;
mod state;
mod tasks;
mod update;
mod view;

use std::time::Duration;

use iced::{Size, Subscription, Task, window};

use message::Message;
use state::State;

const APP_ID: &str = "io.github.ivangor246.gen_music_ai";
const APP_TITLE: &str = "Gen Music AI";
const ICON_SIZE: u32 = 128;

fn subscription(state: &State) -> Subscription<Message> {
    if state.playing {
        iced::time::every(Duration::from_millis(50)).map(|_| Message::Tick)
    } else {
        Subscription::none()
    }
}

pub fn run() -> iced::Result {
    iced::application(APP_TITLE, update::update, view::view)
        .settings(iced::Settings {
            id: Some(APP_ID.to_string()),
            antialiasing: true,
            ..iced::Settings::default()
        })
        .window(window::Settings {
            size: Size::new(1_100.0, 800.0),
            min_size: Some(Size::new(720.0, 600.0)),
            icon: window_icon(),
            ..window::Settings::default()
        })
        .centered()
        .subscription(subscription)
        .run_with(|| (State::new(), Task::none()))
}

fn window_icon() -> Option<window::Icon> {
    window::icon::from_rgba(
        include_bytes!("../../assets/icons/app-icon.rgba").to_vec(),
        ICON_SIZE,
        ICON_SIZE,
    )
    .ok()
}
