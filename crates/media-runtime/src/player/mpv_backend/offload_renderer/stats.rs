//! Display-free render statistics for the offload renderer.

use std::time::Duration;

pub(super) const RENDER_STATS_LOG_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenderStatsSurfacePath {
    DmaBuf,
    Gl,
}

impl RenderStatsSurfacePath {
    pub(super) const fn from_dmabuf_active(is_dmabuf_active: bool) -> Self {
        if is_dmabuf_active {
            Self::DmaBuf
        } else {
            Self::Gl
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::DmaBuf => "DMA-BUF",
            Self::Gl => "GL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RenderStatsReport {
    pub(super) frames_per_second: f64,
    pub(super) rendered_frames: u64,
    pub(super) skipped_frames: u64,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) surface_path: RenderStatsSurfacePath,
}

pub(super) fn render_stats_report_for_interval(
    rendered_frames: u64,
    skipped_frames: u64,
    elapsed: Duration,
    width: i32,
    height: i32,
    surface_path: RenderStatsSurfacePath,
) -> Option<RenderStatsReport> {
    if elapsed < RENDER_STATS_LOG_INTERVAL || rendered_frames == 0 || width <= 0 || height <= 0 {
        return None;
    }

    Some(RenderStatsReport {
        frames_per_second: rendered_frames as f64 / elapsed.as_secs_f64(),
        rendered_frames,
        skipped_frames,
        width,
        height,
        surface_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_stats_waits_for_the_log_interval() {
        assert!(
            render_stats_report_for_interval(
                120,
                3,
                RENDER_STATS_LOG_INTERVAL
                    .checked_sub(Duration::from_millis(1))
                    .expect("one millisecond is below the render stats interval"),
                1920,
                1080,
                RenderStatsSurfacePath::Gl,
            )
            .is_none()
        );
    }

    #[test]
    fn render_stats_reports_at_the_exact_log_interval() {
        let report = render_stats_report_for_interval(
            150,
            0,
            RENDER_STATS_LOG_INTERVAL,
            1920,
            1080,
            RenderStatsSurfacePath::Gl,
        )
        .expect("stats should report exactly at the interval boundary");

        assert_eq!(report.rendered_frames, 150);
        assert_eq!(report.surface_path.label(), "GL");
    }

    #[test]
    fn render_stats_report_preserves_frame_counts_and_surface_size() {
        let report = render_stats_report_for_interval(
            300,
            12,
            Duration::from_secs(10),
            1280,
            720,
            RenderStatsSurfacePath::DmaBuf,
        )
        .expect("stats should report after the interval");

        assert!((report.frames_per_second - 30.0).abs() < f64::EPSILON);
        assert_eq!(report.rendered_frames, 300);
        assert_eq!(report.skipped_frames, 12);
        assert_eq!(report.width, 1280);
        assert_eq!(report.height, 720);
        assert_eq!(report.surface_path.label(), "DMA-BUF");
    }

    #[test]
    fn render_stats_ignores_empty_or_invalid_samples() {
        assert!(
            render_stats_report_for_interval(
                0,
                5,
                Duration::from_secs(10),
                1280,
                720,
                RenderStatsSurfacePath::Gl,
            )
            .is_none()
        );
        assert!(
            render_stats_report_for_interval(
                10,
                0,
                Duration::from_secs(10),
                0,
                720,
                RenderStatsSurfacePath::Gl,
            )
            .is_none()
        );
    }

    #[test]
    fn render_stats_surface_path_tracks_dmabuf_activation() {
        assert_eq!(
            RenderStatsSurfacePath::from_dmabuf_active(true),
            RenderStatsSurfacePath::DmaBuf
        );
        assert_eq!(
            RenderStatsSurfacePath::from_dmabuf_active(false),
            RenderStatsSurfacePath::Gl
        );
    }
}
