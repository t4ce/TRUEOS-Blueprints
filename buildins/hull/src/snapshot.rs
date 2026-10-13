//! Persist the final layout SVG and return its geometry for the live renderer.
use rp5::Document;
use trueos::{async_fs, env, logl};

use super::css::Css;

/// Retained as a demo hook; intentionally has no side effects.
#[allow(non_snake_case, dead_code)]
pub(super) fn uneccecaryStoreToFilesystemForDebug() {}

pub(super) fn save_svg(
    document: &Document,
    css: &Css,
    page_url: &str,
    viewport: (u32, u32),
) -> Result<super::layout::Export, i32> {
    let app_root = env::var("TRUEOS_APP_FS_ROOT").unwrap_or_else(|_| "apps/hull".into());
    let url = url::Url::parse(page_url).map_err(|_| -1)?;
    let host = url.host_str().ok_or(-1)?;
    let root = format!("{app_root}/{host}");
    let started = std::time::Instant::now();
    logl::log(
        logl::level::INFO,
        format_args!(
            "hull: layout start nodes={} sheets={} viewport={}x{}",
            document.nodes.len(),
            css.sheets.len(),
            viewport.0,
            viewport.1,
        ),
    );
    let layout = super::layout::compute(document, css, viewport).map_err(|error| {
        logl::log(
            logl::level::ERROR,
            format_args!("hull: layout failed: {error}"),
        );
        -1
    })?;
    logl::log(
        logl::level::INFO,
        format_args!(
            "hull: layout complete boxes={} elapsed_ms={}",
            layout.line_rects.len(),
            started.elapsed().as_millis(),
        ),
    );
    logl::log(
        logl::level::INFO,
        format_args!("hull: saving {root}/layout.svg"),
    );
    async_fs::block_on(async {
        async_fs::create_dir_all(root.as_bytes()).await?;
        async_fs::write_file(
            format!("{root}/layout.svg").as_bytes(),
            layout.svg.as_bytes(),
        )
        .await
    })?;
    logl::log(
        logl::level::INFO,
        format_args!("hull: saved {root}/layout.svg"),
    );
    Ok(layout)
}
