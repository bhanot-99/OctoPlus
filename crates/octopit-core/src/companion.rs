//! What OctoPit shows about the companion (Phase 12, story C-S06): her rhythm, today's talks and
//! what she remembers, as plain sentences. Pure; no GTK. Status and switches only: there is no
//! chat here and nothing that acts on the computer (DR-1).

use octoplus_proto::v1::{CompanionAction, CompanionStatus};

use crate::gateway::plain;

/// One talk she started herself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TalkRow {
    /// When, in epoch nanoseconds.
    pub at_ns: u64,
    /// What kind of talk, in plain words.
    pub topic: String,
    /// What the owner did with it, in plain words.
    pub reaction: String,
}

/// One thing she remembers about the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRow {
    /// Its id (needed to forget it).
    pub id: u64,
    /// The fact.
    pub text: String,
    /// Kept by chance (true) or because the owner asked (false).
    pub noticed: bool,
}

impl MemoryRow {
    /// "you asked" or "she noticed".
    #[must_use]
    pub const fn how(&self) -> &'static str {
        if self.noticed {
            "she noticed"
        } else {
            "you asked"
        }
    }
}

/// The companion as OctoPit last read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionView {
    /// C5 answered.
    pub reachable: bool,
    /// The companion is switched on.
    pub enabled: bool,
    /// The global pause switch.
    pub paused: bool,
    /// Minutes of quiet the owner asked for that are still running.
    pub quiet_minutes_left: u32,
    /// Talks she started today.
    pub talks_today: u32,
    /// Talks in the last hour.
    pub talks_last_hour: u32,
    /// Daily cap.
    pub per_day_cap: u32,
    /// Hourly cap.
    pub per_hour_max: u32,
    /// Quiet hours start ("23:00").
    pub quiet_start: String,
    /// Quiet hours end ("08:00").
    pub quiet_end: String,
    /// Humour: off, low, normal or high.
    pub humour: String,
    /// 100 is her normal rhythm.
    pub chattiness_percent: u32,
    /// Today's talks, newest first.
    pub talks: Vec<TalkRow>,
    /// What she remembers, newest first.
    pub memories: Vec<MemoryRow>,
    /// How many she kept by chance since the service started.
    pub memories_by_chance: u32,
    /// The memory list could be read.
    pub memory_available: bool,
}

/// What a button in the companion page asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompanionCommand {
    /// Stop starting talks.
    Pause,
    /// Start again.
    Resume,
    /// Talk a bit more.
    ChatMore,
    /// Talk a bit less.
    ChatLess,
    /// Stay quiet for this many minutes.
    QuietFor(u32),
    /// The last joke was funny.
    Funny,
    /// The last joke was not.
    NotFunny,
    /// Forget one remembered fact.
    Forget(u64),
}

impl CompanionCommand {
    /// The message fields for this command: `(action, minutes, id)`.
    #[must_use]
    pub const fn parts(self) -> (CompanionAction, u32, u64) {
        match self {
            Self::Pause => (CompanionAction::Pause, 0, 0),
            Self::Resume => (CompanionAction::Resume, 0, 0),
            Self::ChatMore => (CompanionAction::ChatMore, 0, 0),
            Self::ChatLess => (CompanionAction::ChatLess, 0, 0),
            Self::QuietFor(m) => (CompanionAction::QuietFor, m, 0),
            Self::Funny => (CompanionAction::Funny, 0, 0),
            Self::NotFunny => (CompanionAction::NotFunny, 0, 0),
            Self::Forget(id) => (CompanionAction::Forget, 0, id),
        }
    }
}

/// A topic name in plain words.
#[must_use]
pub fn topic_words(topic: &str) -> &'static str {
    match topic {
        "check_in" => "Check-in",
        "joke" => "A joke",
        "follow_up" => "Following up on something you told her",
        "observation" => "A small remark",
        "fun_fact" => "A fun fact",
        "break_reminder" => "A break reminder",
        "morning" => "Good morning",
        "lunch" => "Lunch time",
        "evening" => "Good evening",
        _ => "A chat",
    }
}

/// What happened to a talk, in plain words.
#[must_use]
pub fn reaction_words(reaction: &str) -> &'static str {
    match reaction {
        "answered" => "you answered",
        "ignored" => "no answer",
        "not_now" => "you said not now",
        _ => "waiting for an answer",
    }
}

impl CompanionView {
    /// What the page shows when C5 cannot be reached.
    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            reachable: false,
            enabled: false,
            paused: false,
            quiet_minutes_left: 0,
            talks_today: 0,
            talks_last_hour: 0,
            per_day_cap: 0,
            per_hour_max: 0,
            quiet_start: String::new(),
            quiet_end: String::new(),
            humour: String::new(),
            chattiness_percent: 100,
            talks: Vec::new(),
            memories: Vec::new(),
            memories_by_chance: 0,
            memory_available: false,
        }
    }

    /// Converts C5's answer.
    #[must_use]
    pub fn from_status(s: &CompanionStatus) -> Self {
        Self {
            reachable: true,
            enabled: s.enabled,
            paused: s.paused,
            quiet_minutes_left: s.quiet_minutes_left,
            talks_today: s.talks_today,
            talks_last_hour: s.talks_last_hour,
            per_day_cap: s.per_day_cap,
            per_hour_max: s.per_hour_max,
            quiet_start: plain(&s.quiet_start, 8),
            quiet_end: plain(&s.quiet_end, 8),
            humour: plain(&s.humour, 8),
            chattiness_percent: s.chattiness_percent,
            talks: s
                .talks
                .iter()
                .map(|t| TalkRow {
                    at_ns: t.at_ns,
                    topic: topic_words(&t.topic).to_owned(),
                    reaction: reaction_words(&t.reaction).to_owned(),
                })
                .collect(),
            memories: s
                .memories
                .iter()
                .map(|m| MemoryRow {
                    id: m.id,
                    text: plain(&m.text, 200),
                    noticed: m.noticed,
                })
                .collect(),
            memories_by_chance: s.memories_by_chance,
            memory_available: s.memory_available,
        }
    }

    /// The one line for the overview card and the page header.
    #[must_use]
    pub fn headline(&self) -> String {
        if !self.reachable {
            "Companion: not reachable".to_owned()
        } else if !self.enabled {
            "Companion: switched off".to_owned()
        } else if self.paused {
            "Companion: paused".to_owned()
        } else if self.quiet_minutes_left > 0 {
            let m = self.quiet_minutes_left;
            if m >= 90 {
                format!("Companion: quiet for about {} more hours", (m + 30) / 60)
            } else {
                format!("Companion: quiet for {m} more minutes")
            }
        } else {
            match self.talks_today {
                0 => "Companion: here, no talks yet today".to_owned(),
                1 => "Companion: here, 1 talk today".to_owned(),
                n => format!("Companion: here, {n} talks today"),
            }
        }
    }

    /// Her rhythm in one plain sentence.
    #[must_use]
    pub fn rhythm_line(&self) -> String {
        if !self.reachable || !self.enabled {
            return String::new();
        }
        format!(
            "Talks at about {}% of her normal rhythm, up to {} an hour and {} a day. Quiet hours {} to {}. Humour: {}.",
            self.chattiness_percent,
            self.per_hour_max,
            self.per_day_cap,
            self.quiet_start,
            self.quiet_end,
            self.humour
        )
    }

    /// The line under an empty memory list.
    #[must_use]
    pub fn memory_note(&self) -> String {
        if !self.reachable {
            "Companion memory is not reachable.".to_owned()
        } else if !self.memory_available {
            "The memory could not be read just now.".to_owned()
        } else if self.memories.is_empty() {
            "She is not holding on to anything about you yet.".to_owned()
        } else {
            format!(
                "{} things. She kept {} by chance since she started; everything else you asked her to keep.",
                self.memories.len(),
                self.memories_by_chance
            )
        }
    }

    /// Whether the pause button should read "Resume".
    #[must_use]
    pub const fn shows_resume(&self) -> bool {
        self.paused
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octoplus_proto::v1::{CompanionMemory, CompanionTalk};

    fn status() -> CompanionStatus {
        CompanionStatus {
            enabled: true,
            talks_today: 3,
            talks_last_hour: 1,
            per_day_cap: 40,
            per_hour_max: 4,
            quiet_start: "23:00".into(),
            quiet_end: "08:00".into(),
            humour: "normal".into(),
            chattiness_percent: 100,
            talks: vec![
                CompanionTalk {
                    at_ns: 10,
                    topic: "morning".into(),
                    reaction: "answered".into(),
                },
                CompanionTalk {
                    at_ns: 5,
                    topic: "joke".into(),
                    reaction: String::new(),
                },
            ],
            memories: vec![
                CompanionMemory {
                    id: 4,
                    text: "my dog is Rex".into(),
                    noticed: true,
                    saved_at_ns: 1,
                },
                CompanionMemory {
                    id: 2,
                    text: "I like tea".into(),
                    noticed: false,
                    saved_at_ns: 0,
                },
            ],
            memories_by_chance: 1,
            memory_available: true,
            ..CompanionStatus::default()
        }
    }

    #[test]
    fn the_status_becomes_plain_rows() {
        let v = CompanionView::from_status(&status());
        assert_eq!(v.talks[0].topic, "Good morning");
        assert_eq!(v.talks[0].reaction, "you answered");
        assert_eq!(v.talks[1].topic, "A joke");
        assert_eq!(v.talks[1].reaction, "waiting for an answer");
        assert_eq!(v.memories[0].how(), "she noticed");
        assert_eq!(v.memories[1].how(), "you asked");
        assert_eq!(v.memories[0].text, "my dog is Rex");
    }

    #[test]
    fn the_headline_says_the_one_thing_that_matters() {
        let mut v = CompanionView::from_status(&status());
        assert_eq!(v.headline(), "Companion: here, 3 talks today");
        v.talks_today = 0;
        assert_eq!(v.headline(), "Companion: here, no talks yet today");
        v.talks_today = 1;
        assert_eq!(v.headline(), "Companion: here, 1 talk today");
        v.quiet_minutes_left = 24;
        assert_eq!(v.headline(), "Companion: quiet for 24 more minutes");
        v.quiet_minutes_left = 120;
        assert_eq!(v.headline(), "Companion: quiet for about 2 more hours");
        v.paused = true;
        assert_eq!(v.headline(), "Companion: paused");
        assert!(v.shows_resume());
        v.enabled = false;
        assert_eq!(v.headline(), "Companion: switched off");
        assert_eq!(
            CompanionView::unavailable().headline(),
            "Companion: not reachable"
        );
    }

    #[test]
    fn the_rhythm_is_one_plain_sentence() {
        let v = CompanionView::from_status(&status());
        assert_eq!(
            v.rhythm_line(),
            "Talks at about 100% of her normal rhythm, up to 4 an hour and 40 a day. Quiet hours 23:00 to 08:00. Humour: normal."
        );
        assert_eq!(CompanionView::unavailable().rhythm_line(), "");
    }

    #[test]
    fn the_memory_note_is_honest_about_every_case() {
        let mut v = CompanionView::from_status(&status());
        assert!(v.memory_note().contains("2 things"));
        assert!(v.memory_note().contains("kept 1 by chance"));
        v.memories.clear();
        assert_eq!(
            v.memory_note(),
            "She is not holding on to anything about you yet."
        );
        v.memory_available = false;
        assert_eq!(v.memory_note(), "The memory could not be read just now.");
        assert_eq!(
            CompanionView::unavailable().memory_note(),
            "Companion memory is not reachable."
        );
    }

    #[test]
    fn commands_map_to_the_messages() {
        assert_eq!(
            CompanionCommand::Pause.parts(),
            (CompanionAction::Pause, 0, 0)
        );
        assert_eq!(
            CompanionCommand::QuietFor(30).parts(),
            (CompanionAction::QuietFor, 30, 0)
        );
        assert_eq!(
            CompanionCommand::Forget(7).parts(),
            (CompanionAction::Forget, 0, 7)
        );
        assert_eq!(
            CompanionCommand::NotFunny.parts().0,
            CompanionAction::NotFunny
        );
    }

    #[test]
    fn text_from_the_part_is_cleaned_before_it_is_shown() {
        let mut s = status();
        s.memories[0].text = "line one\nline two \u{7}bell".into();
        let v = CompanionView::from_status(&s);
        assert!(!v.memories[0].text.contains('\n'));
        assert!(!v.memories[0].text.contains('\u{7}'));
    }
}
