mod density_canvas;
mod message;
mod state;
mod tasks;
mod update;
mod view;

use std::time::Duration;

use iced::{Subscription, Task};

use message::Message;
use state::State;

fn subscription(state: &State) -> Subscription<Message> {
    if state.playing {
        iced::time::every(Duration::from_millis(50)).map(|_| Message::Tick)
    } else {
        Subscription::none()
    }
}

pub fn run() -> iced::Result {
    iced::application(env!("CARGO_PKG_NAME"), update::update, view::view)
        .subscription(subscription)
        .run_with(|| (State::new(), Task::none()))
}
