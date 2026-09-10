use std::cell::{Cell, RefCell};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use adw::{Application, ApplicationWindow, Window};
use gtk::glib;

use big_media_runtime::editor::audio_edit_page::AudioEditPage;
use big_media_runtime::editor::edit_page::EditPage;
use big_media_runtime::settings::Settings;
use big_media_runtime::shared::audio_effects::{
    create_equalizer_dialog, create_nr_settings_dialog,
};

const APPLICATION_ID: &str = "br.com.biglinux.MediaRuntimeUiSurfaceProbe";

fn main() -> glib::ExitCode {
    let application = Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    let _ = relm4::RelmApp::<()>::from_app(application.clone());

    application.connect_activate(|application| match probe_surface().as_str() {
        "audio-editor" => present_audio_editor(application),
        "video-editor" => present_video_editor(application),
        "equalizer" => present_equalizer(application),
        "noise-reduction" => present_noise_reduction(application),
        "leak-cycle" => run_leak_cycle(application),
        other => present_unknown_surface(application, other),
    });

    application.run()
}

fn probe_surface() -> String {
    env::var("BIG_MEDIA_RUNTIME_PROBE_SURFACE").unwrap_or_else(|_| "audio-editor".to_string())
}

fn probe_settings() -> Settings {
    Settings::new("big-media-runtime-ui-probe")
}

fn present_audio_editor(application: &Application) {
    let page = AudioEditPage::new();
    page.set_compact_mode(false);
    present_widget_window(application, "Audio editor", page.widget());
}

fn present_video_editor(application: &Application) {
    let page = EditPage::new();
    present_widget_window(application, "Video editor", page.widget());
}

fn present_widget_window<W>(application: &Application, title: &str, widget: &W)
where
    W: IsA<gtk::Widget>,
{
    let window = ApplicationWindow::builder()
        .application(application)
        .title(title)
        .default_width(1100)
        .default_height(680)
        .content(widget)
        .build();
    window.present();
}

fn present_equalizer(application: &Application) {
    let settings = probe_settings();
    let dialog = create_equalizer_dialog(&settings, None, None);
    dialog.set_application(Some(application));
    dialog.present();
}

fn present_noise_reduction(application: &Application) {
    let settings = probe_settings();
    let dialog = create_nr_settings_dialog(&settings);
    dialog.set_application(Some(application));
    dialog.present();
}

fn present_unknown_surface(application: &Application, surface: &str) {
    let label = gtk::Label::builder()
        .label(format!("Unknown probe surface: {surface}"))
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    present_widget_window(application, "Media runtime probe", &label);
}

#[derive(Default)]
struct LeakCycleState {
    iteration: u32,
    is_dialog_open: bool,
    current_dialog: Option<glib::WeakRef<Window>>,
    closed_samples: Vec<u64>,
}

fn run_leak_cycle(application: &Application) {
    let output_dir = env::var_os("BIG_MEDIA_RUNTIME_PROBE_DIR").map_or_else(
        || env::temp_dir().join("big-media-runtime-leak-probe"),
        PathBuf::from,
    );
    let iterations = env::var("BIG_MEDIA_RUNTIME_LEAK_ITERS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(16);
    let threshold_kib = env::var("BIG_MEDIA_RUNTIME_LEAK_THRESHOLD_KIB")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(512.0);

    fs::create_dir_all(&output_dir).expect("create probe output directory");
    fs::write(
        output_dir.join("samples.csv"),
        "iteration,anonymous_rss_kib\n",
    )
    .expect("write samples header");

    let window = ApplicationWindow::builder()
        .application(application)
        .title("Media runtime leak probe")
        .default_width(480)
        .default_height(240)
        .content(
            &gtk::Label::builder()
                .label("Media runtime leak probe")
                .margin_top(24)
                .margin_bottom(24)
                .margin_start(24)
                .margin_end(24)
                .build(),
        )
        .build();
    window.present();

    let settings = Rc::new(probe_settings());
    let state = Rc::new(RefCell::new(LeakCycleState::default()));
    let completed = Rc::new(Cell::new(false));

    glib::timeout_add_local(Duration::from_millis(90), {
        let application = application.clone();
        let settings = settings.clone();
        let state = state.clone();
        let completed = completed.clone();
        move || {
            if completed.get() {
                return glib::ControlFlow::Break;
            }

            let mut state = state.borrow_mut();
            if state.iteration >= iterations && !state.is_dialog_open {
                write_leak_cycle_result(&output_dir, &state.closed_samples, threshold_kib);
                completed.set(true);
                application.quit();
                return glib::ControlFlow::Break;
            }

            if state.is_dialog_open {
                if let Some(dialog) = state
                    .current_dialog
                    .take()
                    .and_then(|dialog| dialog.upgrade())
                {
                    dialog.close();
                }
                let sample = anonymous_rss_kib();
                append_sample(&output_dir, state.iteration, sample);
                state.closed_samples.push(sample);
                state.iteration += 1;
                state.is_dialog_open = false;
                return glib::ControlFlow::Continue;
            }

            let dialog = if state.iteration % 2 == 0 {
                create_equalizer_dialog(&settings, None, None)
            } else {
                create_nr_settings_dialog(&settings)
            };
            dialog.set_application(Some(&application));
            dialog.present();
            state.current_dialog = Some(dialog.downgrade());
            state.is_dialog_open = true;
            glib::ControlFlow::Continue
        }
    });
}

fn anonymous_rss_kib() -> u64 {
    let Ok(content) = fs::read_to_string("/proc/self/smaps_rollup") else {
        return 0;
    };
    content
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            (parts.next()? == "Anonymous:").then(|| parts.next()?.parse::<u64>().ok())?
        })
        .unwrap_or(0)
}

fn append_sample(output_dir: &Path, iteration: u32, sample_kib: u64) {
    use std::io::Write as _;

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(output_dir.join("samples.csv"))
        .expect("open samples csv");
    writeln!(file, "{iteration},{sample_kib}").expect("write sample row");
}

fn write_leak_cycle_result(output_dir: &Path, samples: &[u64], threshold_kib: f64) {
    let first = samples.first().copied().unwrap_or(0);
    let last = samples.last().copied().unwrap_or(0);
    let slope = if samples.len() > 1 {
        (last as f64 - first as f64) / (samples.len() as f64 - 1.0)
    } else {
        0.0
    };
    let status = if slope <= threshold_kib {
        "clean"
    } else {
        "growth"
    };

    let text = format!(
        "anon RSS after close: first={first}KiB last={last}KiB over {} iters\nper-iteration slope = {slope:.0} KiB/open\nVERDICT: {status}\n",
        samples.len()
    );
    fs::write(output_dir.join("probe-result.txt"), text).expect("write probe result");

    let json = serde_json::json!({
        "status": status,
        "closed_samples": samples.len(),
        "first_kib": first,
        "last_kib": last,
        "slope_kib_per_open": slope.round(),
        "threshold_kib_per_open": threshold_kib,
    });
    fs::write(
        output_dir.join("probe-results.json"),
        serde_json::to_string_pretty(&json).expect("serialize probe result"),
    )
    .expect("write probe result json");
}
