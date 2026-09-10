use super::super::mime::guess_mime;

#[test]
fn test_guess_mime_video_formats() {
    assert_eq!(guess_mime("movie.webm"), "video/webm");
    assert_eq!(guess_mime("movie.mkv"), "video/x-matroska");
    assert_eq!(guess_mime("movie.avi"), "video/x-msvideo");
    assert_eq!(guess_mime("movie.mov"), "video/quicktime");
    assert_eq!(guess_mime("movie.flv"), "video/x-flv");
    assert_eq!(guess_mime("movie.mp4"), "video/mp4");
}

#[test]
fn test_guess_mime_transport_stream_variants() {
    assert_eq!(guess_mime("clip.ts"), "video/mp2t");
    assert_eq!(guess_mime("clip.m2ts"), "video/mp2t");
    assert_eq!(guess_mime("clip.mts"), "video/mp2t");
}

#[test]
fn test_guess_mime_audio_formats() {
    assert_eq!(guess_mime("song.mp3"), "audio/mpeg");
    assert_eq!(guess_mime("song.flac"), "audio/flac");
    assert_eq!(guess_mime("song.ogg"), "audio/ogg");
    assert_eq!(guess_mime("song.oga"), "audio/ogg");
    assert_eq!(guess_mime("song.wav"), "audio/wav");
    assert_eq!(guess_mime("song.m4a"), "audio/mp4");
    assert_eq!(guess_mime("song.aac"), "audio/mp4");
}

#[test]
fn test_guess_mime_case_insensitive() {
    assert_eq!(guess_mime("VIDEO.MKV"), "video/x-matroska");
    assert_eq!(guess_mime("Music.FLAC"), "audio/flac");
    assert_eq!(guess_mime("file.Mp4"), "video/mp4");
}

#[test]
fn test_guess_mime_unknown_ext_defaults_to_mp4() {
    assert_eq!(guess_mime("file.xyz"), "video/mp4");
    assert_eq!(guess_mime("no_extension"), "video/mp4");
    assert_eq!(guess_mime(""), "video/mp4");
}

#[test]
fn test_guess_mime_multiple_dots_uses_last_ext() {
    assert_eq!(guess_mime("file.backup.mkv"), "video/x-matroska");
    assert_eq!(guess_mime("my.movie.2024.webm"), "video/webm");
}

#[test]
fn test_guess_mime_full_path() {
    assert_eq!(guess_mime("/home/user/Videos/clip.flv"), "video/x-flv");
    assert_eq!(guess_mime("/mnt/nas/music/song.flac"), "audio/flac");
}
