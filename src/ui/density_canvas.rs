//! Note-density visualization with click-to-seek, mirroring the Tkinter
//! `TrackVisualization`.

use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Program, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Theme, mouse};

use super::message::Message;

pub struct DensityCanvas<'a> {
    pub cache: &'a canvas::Cache,
    pub density: &'a [f32],
    pub position_fraction: f32,
}

impl Program<Message> for DensityCanvas<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let bars = self.cache.draw(renderer, bounds.size(), |frame| {
            let width = frame.width();
            let height = frame.height();
            let middle = height / 2.0;
            frame.stroke(
                &Path::line(Point::new(0.0, middle), Point::new(width, middle)),
                Stroke::default()
                    .with_color(Color::from_rgb8(0x40, 0x52, 0x61))
                    .with_width(1.0),
            );
            if !self.density.is_empty() {
                let bar_width = width / self.density.len() as f32;
                for (i, &value) in self.density.iter().enumerate() {
                    let amplitude = (value * (height / 2.0 - 7.0)).max(1.0);
                    let x = (i as f32 + 0.5) * bar_width;
                    frame.stroke(
                        &Path::line(
                            Point::new(x, middle - amplitude),
                            Point::new(x, middle + amplitude),
                        ),
                        Stroke::default()
                            .with_color(Color::from_rgb8(0x4f, 0xa3, 0xd1))
                            .with_width(2.0),
                    );
                }
            }
        });

        let mut overlay = Frame::new(renderer, bounds.size());
        let x = self.position_fraction.clamp(0.0, 1.0) * overlay.width();
        overlay.stroke(
            &Path::line(Point::new(x, 0.0), Point::new(x, overlay.height())),
            Stroke::default()
                .with_color(Color::from_rgb8(0xf6, 0xc8, 0x5f))
                .with_width(2.0),
        );

        vec![bars, overlay.into_geometry()]
    }

    fn update(
        &self,
        _state: &mut (),
        event: Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if let Some(position) = cursor.position_in(bounds) {
                let fraction = (position.x / bounds.width).clamp(0.0, 1.0);
                return (
                    canvas::event::Status::Captured,
                    Some(Message::Seek(fraction)),
                );
            }
        }
        (canvas::event::Status::Ignored, None)
    }
}
