use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

pub fn create_pbar(total: u64, message: &'static str) -> ProgressBar {
    let pbar = ProgressBar::new(total);
    set_pbar_options(pbar, message, true)
}

pub fn set_pbar_options(pbar: ProgressBar, message: &'static str, tick: bool) -> ProgressBar {
    pbar.set_style(
        ProgressStyle::with_template("{msg} {wide_bar} {pos}/{len} ({per_sec})").unwrap(),
    );
    if !pbar.is_hidden() && tick {
        pbar.enable_steady_tick(Duration::from_secs(1));
    }
    pbar.set_message(message);
    pbar
}
