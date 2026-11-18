use crate::Actor;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

/// A project-scoped trigger. It creates a Task from a workflow, input and
/// templates, either on a schedule or when a user clicks "run now".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trigger {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub workflow_id: String,
    #[serde(default)]
    pub input: Value,
    /// `manual`, `every:<minutes>` or `daily@HH:MM` (UTC).
    pub schedule: String,
    pub enabled: bool,
    pub title_template: String,
    pub description_template: String,
    pub created_by: Actor,
    pub created_by_id: Uuid,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub next_run_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_run_at: Option<OffsetDateTime>,
    pub version: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl Trigger {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: Uuid,
        name: impl Into<String>,
        description: impl Into<String>,
        workflow_id: impl Into<String>,
        input: Value,
        schedule: impl Into<String>,
        title_template: impl Into<String>,
        description_template: impl Into<String>,
        created_by: Actor,
        created_by_id: Uuid,
    ) -> Result<Self, TriggerError> {
        let now = OffsetDateTime::now_utc();
        let mut trigger = Self {
            id: Uuid::now_v7(),
            project_id,
            name: name.into().trim().to_string(),
            description: description.into().trim().to_string(),
            workflow_id: workflow_id.into().trim().to_string(),
            input,
            schedule: schedule.into().trim().to_string(),
            enabled: true,
            title_template: title_template.into().trim().to_string(),
            description_template: description_template.into().trim().to_string(),
            created_by,
            created_by_id,
            next_run_at: None,
            last_run_at: None,
            version: 1,
            created_at: now,
            updated_at: now,
        };
        trigger.validate()?;
        trigger.next_run_at = TriggerSchedule::parse(&trigger.schedule)?.next_after(now)?;
        Ok(trigger)
    }

    pub fn validate(&self) -> Result<(), TriggerError> {
        let mut errors = Vec::new();
        if self.project_id.is_nil() {
            errors.push("project is required".to_string());
        }
        if self.name.is_empty() {
            errors.push("trigger name is required".to_string());
        }
        if self.workflow_id.is_empty() {
            errors.push("trigger workflow is required".to_string());
        }
        if self.title_template.is_empty() {
            errors.push("trigger title template is required".to_string());
        }
        if TriggerSchedule::parse(&self.schedule)
            .and_then(|schedule| schedule.next_after(OffsetDateTime::now_utc()))
            .is_err()
        {
            errors.push(format!("invalid trigger schedule: {}", self.schedule));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(TriggerError(errors))
        }
    }

    pub fn touch(&mut self) {
        self.version = self.version.saturating_add(1);
        self.updated_at = OffsetDateTime::now_utc();
    }
}

/// Parsed trigger schedule. The persisted form stays a string so the model
/// does not depend on a cron library yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerSchedule {
    Manual,
    EveryMinutes(u64),
    DailyAt { hour: u8, minute: u8 },
}

impl TriggerSchedule {
    pub fn parse(value: &str) -> Result<Self, TriggerError> {
        let value = value.trim();
        if value == "manual" {
            return Ok(Self::Manual);
        }
        if let Some(rest) = value.strip_prefix("every:") {
            let minutes = rest
                .parse::<u64>()
                .map_err(|_| TriggerError(vec![format!("invalid interval: {value}")]))?;
            if minutes == 0 {
                return Err(TriggerError(vec![
                    "interval must be at least one minute".to_string(),
                ]));
            }
            return Ok(Self::EveryMinutes(minutes));
        }
        if let Some(rest) = value.strip_prefix("daily@") {
            let (hour, minute) = rest
                .split_once(':')
                .ok_or_else(|| TriggerError(vec![format!("invalid daily schedule: {value}")]))?;
            let hour = hour
                .parse::<u8>()
                .map_err(|_| TriggerError(vec![format!("invalid hour: {value}")]))?;
            let minute = minute
                .parse::<u8>()
                .map_err(|_| TriggerError(vec![format!("invalid minute: {value}")]))?;
            if hour > 23 || minute > 59 {
                return Err(TriggerError(vec![format!(
                    "invalid daily time: {value}"
                )]));
            }
            return Ok(Self::DailyAt { hour, minute });
        }
        Err(TriggerError(vec![format!("unknown schedule: {value}")]))
    }

    pub fn next_after(&self, now: OffsetDateTime) -> Result<Option<OffsetDateTime>, TriggerError> {
        let now = now.to_offset(time::UtcOffset::UTC);
        Ok(match self {
            Self::Manual => None,
            Self::EveryMinutes(minutes) => {
                Some(now + time::Duration::minutes(*minutes as i64))
            }
            Self::DailyAt { hour, minute } => {
                let time = time::Time::from_hms(*hour, *minute, 0)
                    .map_err(|error| TriggerError(vec![error.to_string()]))?;
                let mut date = now.date();
                if now.time() >= time {
                    date = date
                        .next_day()
                        .ok_or_else(|| TriggerError(vec!["date overflow".to_string()]))?;
                }
                let candidate = date.with_time(time).assume_utc();
                Some(candidate)
            }
        })
    }
}

impl std::str::FromStr for TriggerSchedule {
    type Err = TriggerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("trigger validation failed: {}", .0.join("; "))]
pub struct TriggerError(pub Vec<String>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerRunReason {
    Scheduled,
    Manual,
}

impl TriggerRunReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "scheduled" => Some(Self::Scheduled),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerRun {
    pub id: Uuid,
    pub trigger_id: Uuid,
    pub task_id: Uuid,
    pub reason: TriggerRunReason,
    pub triggered_by: Actor,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_schedules() {
        assert_eq!(
            TriggerSchedule::parse("manual").unwrap(),
            TriggerSchedule::Manual
        );
        assert_eq!(
            TriggerSchedule::parse("every:60").unwrap(),
            TriggerSchedule::EveryMinutes(60)
        );
        assert_eq!(
            TriggerSchedule::parse("daily@09:30").unwrap(),
            TriggerSchedule::DailyAt {
                hour: 9,
                minute: 30
            }
        );
        assert!(TriggerSchedule::parse("every:0").is_err());
        assert!(TriggerSchedule::parse("daily@25:00").is_err());
        assert!(TriggerSchedule::parse("cron").is_err());
    }

    #[test]
    fn computes_next_runs() {
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        assert_eq!(TriggerSchedule::Manual.next_after(now).unwrap(), None);
        assert!(
            TriggerSchedule::EveryMinutes(30)
                .next_after(now)
                .unwrap()
                .is_some()
        );
        assert!(
            TriggerSchedule::DailyAt {
                hour: 9,
                minute: 0
            }
            .next_after(now)
            .unwrap()
            .is_some()
        );
    }
}
