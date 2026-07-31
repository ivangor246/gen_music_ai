//! Application palette and reusable widget styles.

use iced::widget::{button, checkbox, container, pick_list, progress_bar, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

pub const SPACE_XS: u16 = 4;
pub const SPACE_SM: u16 = 8;
pub const SPACE_MD: u16 = 16;
pub const CONTROL_RADIUS: f32 = 8.0;
pub const CARD_RADIUS: f32 = 12.0;

pub fn application() -> Theme {
    Theme::custom(
        "Gen Music AI".to_string(),
        iced::theme::Palette {
            background: Color::from_rgb8(7, 11, 26),
            text: Color::from_rgb8(235, 241, 255),
            primary: Color::from_rgb8(51, 217, 230),
            success: Color::from_rgb8(77, 214, 151),
            danger: Color::from_rgb8(255, 105, 135),
        },
    )
}

pub fn card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb8(17, 24, 44))),
        border: Border {
            color: Color::from_rgb8(32, 44, 74),
            width: 1.0,
            radius: CARD_RADIUS.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.28),
            offset: Vector::new(0.0, 3.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}

pub fn inset_card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb8(10, 16, 32))),
        border: Border {
            color: Color::from_rgb8(28, 39, 67),
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        ..container::Style::default()
    }
}

pub fn selected_card(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb8(14, 29, 51))),
        border: Border {
            color: theme.palette().primary,
            width: 1.5,
            radius: CONTROL_RADIUS.into(),
        },
        ..container::Style::default()
    }
}

pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    rounded_button(button::primary(theme, status), status)
}

pub fn secondary_button(theme: &Theme, status: button::Status) -> button::Style {
    rounded_button(button::secondary(theme, status), status)
}

pub fn danger_button(theme: &Theme, status: button::Status) -> button::Style {
    rounded_button(button::danger(theme, status), status)
}

pub fn tag_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let pair = match status {
        button::Status::Hovered => palette.primary.base,
        button::Status::Disabled => palette.secondary.weak,
        button::Status::Active | button::Status::Pressed => palette.primary.weak,
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            color: theme.palette().primary,
            width: 1.0,
            radius: CONTROL_RADIUS.into(),
        },
        ..button::Style::default()
    }
}

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = text_input::default(theme, status);
    style.border.radius = CONTROL_RADIUS.into();
    style
}

pub fn selection(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let mut style = pick_list::default(theme, status);
    style.border.radius = CONTROL_RADIUS.into();
    style
}

pub fn check(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let mut style = checkbox::primary(theme, status);
    style.border.radius = 4.0.into();
    style
}

pub fn progress(theme: &Theme) -> progress_bar::Style {
    let mut style = progress_bar::primary(theme);
    style.border.radius = CONTROL_RADIUS.into();
    style
}

fn rounded_button(mut style: button::Style, status: button::Status) -> button::Style {
    style.border.radius = CONTROL_RADIUS.into();
    if matches!(status, button::Status::Hovered) {
        style.shadow = Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.25),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        };
    }
    style
}
