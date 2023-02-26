use log::warn;
use std::time::Instant;

pub fn log_timer(
    state: &'static str,
    path: &str,
    previous_instant: Option<(&'static str, Instant)>,
) -> Option<(&'static str, Instant)> {
    match previous_instant {
        None => warn!("[{}] {state}", path),
        Some((prev_state, p)) => warn!(
            "[{}] Started: {state}. {prev_state} finished in {}s",
            path,
            p.elapsed().as_secs()
        ),
    }
    Some((state, Instant::now()))
}
