//! `big-video-player` binary entrypoint; delegates to the library [`run`].

fn main() -> gtk::glib::ExitCode {
    big_video_player::run()
}
