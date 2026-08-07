fn main() -> iced::Result {
    // Cap tensor-math threads before anything can spawn work on them.
    gen_music_ai::runtime::configure_threads();
    gen_music_ai::ui::run()
}
