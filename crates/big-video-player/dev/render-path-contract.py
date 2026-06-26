#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Guard the main video path against CPU-copy regressions."""

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
RENDER_SOURCE = REPO_ROOT / "src/video/ui/video_shell/render.rs"
LIB_SOURCE = REPO_ROOT / "src/lib.rs"

REQUIRED_RENDER_TOKENS = (
    "FastRenderContext",
    "video_surface::install_best_backend",
    "video_surface::request_offload_render",
    "video_surface::create_render_context",
    "video_surface::render_glarea_frame",
    "gtk::GLArea",
    "gl_area.set_auto_render(false)",
    "gtk::gdk::GLContext::clear_current()",
)

REQUIRED_LIB_TOKENS = (
    "nvidia_proprietary_driver_in_use()",
    'std::env::set_var("GSK_RENDERER", gsk_renderer)',
    '"ngl"',
    '"gl"',
    "GLArea video path",
    "GraphicsOffload zero-copy",
)

SOFTWARE_FALLBACK_START = "/// Software (no-GL) video path"

FORBIDDEN_MAIN_RENDER_TOKENS = (
    "gdk::MemoryTexture",
    "MemoryTexture::new",
    "gdk::Pixbuf",
    "Texture::for_pixbuf",
    "from_bytes",
    "glReadPixels",
    "read_pixels",
    "screenshot_to_file",
    "RGB buffers",
)


def read_source(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise SystemExit(f"failed to read {path}: {error}") from error


def collect_missing_tokens(
    source_text: str, required_tokens: tuple[str, ...]
) -> list[str]:
    return [token for token in required_tokens if token not in source_text]


def collect_forbidden_tokens(source_text: str) -> list[str]:
    return [token for token in FORBIDDEN_MAIN_RENDER_TOKENS if token in source_text]


def main_render_source(source_text: str) -> str:
    fallback_index = source_text.find(SOFTWARE_FALLBACK_START)
    if fallback_index == -1:
        return source_text
    return source_text[:fallback_index]


def main() -> int:
    render_source = read_source(RENDER_SOURCE)
    lib_source = read_source(LIB_SOURCE)

    failures: list[str] = []
    missing_render_tokens = collect_missing_tokens(
        render_source, REQUIRED_RENDER_TOKENS
    )
    if missing_render_tokens:
        failures.append(
            "render.rs is missing direct/offload render tokens: "
            + ", ".join(missing_render_tokens)
        )

    forbidden_render_tokens = collect_forbidden_tokens(
        main_render_source(render_source)
    )
    if forbidden_render_tokens:
        failures.append(
            "render.rs contains forbidden primary-path CPU-copy tokens: "
            + ", ".join(forbidden_render_tokens)
        )

    missing_lib_tokens = collect_missing_tokens(lib_source, REQUIRED_LIB_TOKENS)
    if missing_lib_tokens:
        failures.append(
            "src/lib.rs is missing video backend policy tokens: "
            + ", ".join(missing_lib_tokens)
        )

    if failures:
        print("video render path contract failed", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    print("video render path contract passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
