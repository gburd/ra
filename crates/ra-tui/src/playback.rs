//! Playback control state for timeline stepping.
//!
//! Manages play/pause, speed control, and auto-advance timing
//! independently of the app state machine, so it can be tested
//! and reused in isolation.

use std::time::{Duration, Instant};

/// Playback speed presets as (label, tick-interval-millis) pairs.
const SPEEDS: &[(&str, u64)] = &[
    ("0.25x", 4000),
    ("0.5x", 2000),
    ("1x", 1000),
    ("2x", 500),
    ("4x", 250),
    ("8x", 125),
];

/// Playback controller for timeline stepping.
#[derive(Debug)]
pub struct PlaybackController {
    /// Whether auto-play is active.
    playing: bool,
    /// Index into the `SPEEDS` array.
    speed_index: usize,
    /// Last auto-advance time.
    last_advance: Instant,
    /// Current step position.
    current_step: usize,
    /// Total number of steps.
    total_steps: usize,
}

impl PlaybackController {
    /// Create a new controller for a timeline with `total_steps`
    /// snapshots.
    #[must_use]
    pub fn new(total_steps: usize) -> Self {
        Self {
            playing: false,
            speed_index: 2, // 1x
            last_advance: Instant::now(),
            current_step: 0,
            total_steps,
        }
    }

    /// Whether auto-play is active.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Current step index.
    #[must_use]
    pub fn current_step(&self) -> usize {
        self.current_step
    }

    /// Total number of steps.
    #[must_use]
    pub fn total_steps(&self) -> usize {
        self.total_steps
    }

    /// Current speed label (e.g., "1x").
    #[must_use]
    pub fn speed_label(&self) -> &str {
        SPEEDS[self.speed_index].0
    }

    /// Current speed index.
    #[must_use]
    pub fn speed_index(&self) -> usize {
        self.speed_index
    }

    /// Tick duration for the current speed.
    #[must_use]
    pub fn tick_duration(&self) -> Duration {
        Duration::from_millis(SPEEDS[self.speed_index].1)
    }

    /// Toggle play/pause.
    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
        if self.playing {
            self.last_advance = Instant::now();
        }
    }

    /// Start playback.
    pub fn play(&mut self) {
        self.playing = true;
        self.last_advance = Instant::now();
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Increase playback speed.
    pub fn speed_up(&mut self) {
        if self.speed_index < SPEEDS.len() - 1 {
            self.speed_index += 1;
        }
    }

    /// Decrease playback speed.
    pub fn slow_down(&mut self) {
        if self.speed_index > 0 {
            self.speed_index -= 1;
        }
    }

    /// Step forward by one. Returns true if step changed.
    pub fn step_forward(&mut self) -> bool {
        let max = self.total_steps.saturating_sub(1);
        if self.current_step < max {
            self.current_step += 1;
            true
        } else {
            self.playing = false;
            false
        }
    }

    /// Step backward by one. Returns true if step changed.
    pub fn step_backward(&mut self) -> bool {
        if self.current_step > 0 {
            self.current_step -= 1;
            true
        } else {
            false
        }
    }

    /// Jump to the first step.
    pub fn jump_first(&mut self) {
        self.current_step = 0;
    }

    /// Jump to the last step.
    pub fn jump_last(&mut self) {
        self.current_step = self.total_steps.saturating_sub(1);
    }

    /// Seek to a specific step. Clamps to valid range.
    pub fn seek(&mut self, step: usize) {
        self.current_step =
            step.min(self.total_steps.saturating_sub(1));
    }

    /// Check auto-advance tick. Returns true if a step was advanced.
    pub fn tick(&mut self) -> bool {
        if self.playing
            && self.last_advance.elapsed() >= self.tick_duration()
        {
            let advanced = self.step_forward();
            self.last_advance = Instant::now();
            return advanced;
        }
        false
    }

    /// Whether there are more steps ahead.
    #[must_use]
    pub fn has_next(&self) -> bool {
        self.current_step < self.total_steps.saturating_sub(1)
    }

    /// Whether there are steps behind.
    #[must_use]
    pub fn has_previous(&self) -> bool {
        self.current_step > 0
    }

    /// Progress fraction (0.0 to 1.0).
    #[must_use]
    pub fn progress(&self) -> f64 {
        if self.total_steps <= 1 {
            return 1.0;
        }
        self.current_step as f64
            / (self.total_steps - 1) as f64
    }

    /// Update total steps (when loading a new timeline).
    pub fn set_total_steps(&mut self, total: usize) {
        self.total_steps = total;
        if self.current_step >= total {
            self.current_step = total.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_controller_starts_paused() {
        let ctrl = PlaybackController::new(5);
        assert!(!ctrl.is_playing());
        assert_eq!(ctrl.current_step(), 0);
        assert_eq!(ctrl.total_steps(), 5);
    }

    #[test]
    fn default_speed_is_1x() {
        let ctrl = PlaybackController::new(5);
        assert_eq!(ctrl.speed_label(), "1x");
    }

    #[test]
    fn toggle_play() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.toggle_play();
        assert!(ctrl.is_playing());
        ctrl.toggle_play();
        assert!(!ctrl.is_playing());
    }

    #[test]
    fn play_and_pause() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.play();
        assert!(ctrl.is_playing());
        ctrl.pause();
        assert!(!ctrl.is_playing());
    }

    #[test]
    fn step_forward() {
        let mut ctrl = PlaybackController::new(5);
        assert!(ctrl.step_forward());
        assert_eq!(ctrl.current_step(), 1);
    }

    #[test]
    fn step_forward_at_end() {
        let mut ctrl = PlaybackController::new(3);
        ctrl.step_forward();
        ctrl.step_forward();
        assert!(!ctrl.step_forward());
        assert_eq!(ctrl.current_step(), 2);
    }

    #[test]
    fn step_forward_at_end_pauses() {
        let mut ctrl = PlaybackController::new(2);
        ctrl.play();
        ctrl.step_forward();
        // At step 1 (last), step_forward returns false and pauses
        assert!(!ctrl.step_forward());
        assert!(!ctrl.is_playing());
    }

    #[test]
    fn step_backward() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.step_forward();
        ctrl.step_forward();
        assert!(ctrl.step_backward());
        assert_eq!(ctrl.current_step(), 1);
    }

    #[test]
    fn step_backward_at_start() {
        let ctrl = PlaybackController::new(5);
        let mut ctrl = ctrl;
        assert!(!ctrl.step_backward());
        assert_eq!(ctrl.current_step(), 0);
    }

    #[test]
    fn jump_first() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.step_forward();
        ctrl.step_forward();
        ctrl.jump_first();
        assert_eq!(ctrl.current_step(), 0);
    }

    #[test]
    fn jump_last() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.jump_last();
        assert_eq!(ctrl.current_step(), 4);
    }

    #[test]
    fn seek_valid() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.seek(3);
        assert_eq!(ctrl.current_step(), 3);
    }

    #[test]
    fn seek_out_of_bounds_clamps() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.seek(100);
        assert_eq!(ctrl.current_step(), 4);
    }

    #[test]
    fn speed_up() {
        let mut ctrl = PlaybackController::new(5);
        let initial = ctrl.speed_index();
        ctrl.speed_up();
        assert_eq!(ctrl.speed_index(), initial + 1);
    }

    #[test]
    fn speed_up_at_max() {
        let mut ctrl = PlaybackController::new(5);
        for _ in 0..20 {
            ctrl.speed_up();
        }
        assert_eq!(ctrl.speed_index(), SPEEDS.len() - 1);
    }

    #[test]
    fn slow_down() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.speed_up();
        ctrl.speed_up();
        ctrl.slow_down();
        assert_eq!(ctrl.speed_index(), 3);
    }

    #[test]
    fn slow_down_at_min() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.slow_down();
        ctrl.slow_down();
        ctrl.slow_down();
        assert_eq!(ctrl.speed_index(), 0);
    }

    #[test]
    fn has_next_true() {
        let ctrl = PlaybackController::new(5);
        assert!(ctrl.has_next());
    }

    #[test]
    fn has_next_false_at_end() {
        let mut ctrl = PlaybackController::new(2);
        ctrl.jump_last();
        assert!(!ctrl.has_next());
    }

    #[test]
    fn has_previous_false_at_start() {
        let ctrl = PlaybackController::new(5);
        assert!(!ctrl.has_previous());
    }

    #[test]
    fn has_previous_true() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.step_forward();
        assert!(ctrl.has_previous());
    }

    #[test]
    fn progress_at_start() {
        let ctrl = PlaybackController::new(5);
        assert!((ctrl.progress() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_at_end() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.jump_last();
        assert!((ctrl.progress() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_at_middle() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.seek(2);
        assert!((ctrl.progress() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_single_step() {
        let ctrl = PlaybackController::new(1);
        assert!((ctrl.progress() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn set_total_steps_preserves_position() {
        let mut ctrl = PlaybackController::new(10);
        ctrl.seek(5);
        ctrl.set_total_steps(20);
        assert_eq!(ctrl.current_step(), 5);
        assert_eq!(ctrl.total_steps(), 20);
    }

    #[test]
    fn set_total_steps_clamps_position() {
        let mut ctrl = PlaybackController::new(10);
        ctrl.seek(8);
        ctrl.set_total_steps(5);
        assert_eq!(ctrl.current_step(), 4);
    }

    #[test]
    fn tick_when_paused_does_nothing() {
        let mut ctrl = PlaybackController::new(5);
        assert!(!ctrl.tick());
        assert_eq!(ctrl.current_step(), 0);
    }

    #[test]
    fn tick_duration_varies_with_speed() {
        let mut ctrl = PlaybackController::new(5);
        let base = ctrl.tick_duration();
        ctrl.speed_up();
        let faster = ctrl.tick_duration();
        assert!(faster < base);
    }

    #[test]
    fn single_step_timeline() {
        let mut ctrl = PlaybackController::new(1);
        assert!(!ctrl.has_next());
        assert!(!ctrl.has_previous());
        assert!(!ctrl.step_forward());
        assert!(!ctrl.step_backward());
    }

    #[test]
    fn zero_step_timeline() {
        let ctrl = PlaybackController::new(0);
        assert!(!ctrl.has_next());
        assert!(!ctrl.has_previous());
        assert_eq!(ctrl.current_step(), 0);
    }

    #[test]
    fn forward_backward_roundtrip() {
        let mut ctrl = PlaybackController::new(5);
        ctrl.step_forward();
        ctrl.step_forward();
        ctrl.step_backward();
        assert_eq!(ctrl.current_step(), 1);
    }
}
