//! `big-audio-player` binary entrypoint: thin wrapper over the library crate.

fn main() -> gtk::glib::ExitCode {
    big_audio_player::run()
}
