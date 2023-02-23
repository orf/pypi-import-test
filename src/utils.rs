use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

pub fn create_pbar(total: u64, message: &'static str) -> ProgressBar {
    let pbar = ProgressBar::new(total);
    pbar.set_style(
        ProgressStyle::with_template("{msg} {wide_bar} {pos}/{len} ({per_sec})").unwrap(),
    );
    pbar.enable_steady_tick(Duration::from_secs(1));
    pbar.set_message(message);
    pbar
}
