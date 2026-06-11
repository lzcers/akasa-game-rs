use std::{env, time::Duration};

#[derive(Clone, Copy, Debug)]
pub struct AgentTaskConfig {
    pub(super) task_timeout: Duration,
    pub(super) initial_output_timeout: Duration,
    pub(super) no_progress_timeout: Duration,
    max_retries: usize,
}

const DEFAULT_TASK_TIMEOUT_SECS: u64 = 300;
const DEFAULT_INITIAL_OUTPUT_TIMEOUT_SECS: u64 = 45;
const DEFAULT_NO_PROGRESS_TIMEOUT_SECS: u64 = 90;
const DEFAULT_MAX_RETRIES: usize = 2;

impl Default for AgentTaskConfig {
    fn default() -> Self {
        Self {
            task_timeout: Duration::from_secs(DEFAULT_TASK_TIMEOUT_SECS),
            initial_output_timeout: Duration::from_secs(DEFAULT_INITIAL_OUTPUT_TIMEOUT_SECS),
            no_progress_timeout: Duration::from_secs(DEFAULT_NO_PROGRESS_TIMEOUT_SECS),
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

impl AgentTaskConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            task_timeout: env_duration_secs("AKASHIC_TASK_TIMEOUT_SECS", defaults.task_timeout),
            initial_output_timeout: env_duration_secs(
                "AKASHIC_TASK_INITIAL_OUTPUT_TIMEOUT_SECS",
                defaults.initial_output_timeout,
            ),
            no_progress_timeout: env_duration_secs(
                "AKASHIC_TASK_NO_PROGRESS_TIMEOUT_SECS",
                defaults.no_progress_timeout,
            ),
            max_retries: env_usize("AKASHIC_TASK_MAX_RETRIES", defaults.max_retries),
        }
    }

    pub(super) fn max_attempts(self) -> usize {
        self.max_retries.saturating_add(1).max(1)
    }
}

fn env_duration_secs(name: &str, default: Duration) -> Duration {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_secs)
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}
