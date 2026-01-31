//! Topics of things (MemoryService.md MEM-TOP-1 to MEM-TOP-7, amended January 2026 (user decision;
//! Approved by the user January 2026)): a short plain label per thing of a place, kept in this
//! service's own table `memory_thing_topics`, beside the memory component and never in it.

use memory::service::ListEntitiesRequest;
use memory::types::ports::PiiDetector;
use ordin_types::time::Timestamp;
use ordin_types::MemoryThingId;
use serde_json::json;

use crate::db::q;
use crate::error::{map_call_error, MemoryError};
use crate::model::{MemoryPlace, ThingTopic, ThingTopicsView, TopicSetBy};
use crate::service::{new_correlation_id, Inner, MemoryService};
use crate::store;

/// MEM-TOP-2: a topic's most characters.
pub const TOPIC_MAX_CHARS: usize = 40;
/// MEM-TOP-4: the most things a read returns.
const THINGS_MAX: i64 = 5_000;
/// MEM-TOP-4: the most distinct topics a read returns.
const TOPICS_MAX: usize = 200;
/// MEM-TOP-3: the most waiting topics one read settles.
const HINTS_PER_READ: i64 = 100;
/// MEM-TOP-3: a waiting topic whose thing never appeared is dropped after this many days.
const HINT_DAYS: i64 = 30;

/// MEM-TOP-2: 1–40 characters once trimmed, no control character.
// spec: ordin-desktop/components/MemoryService#MEM-TOP-2
pub(crate) fn topic_ok(topic: &str) -> bool {
    (1..=TOPIC_MAX_CHARS).contains(&topic.trim().chars().count())
        && !topic.chars().any(char::is_control)
}

fn workspace_of(place: MemoryPlace) -> Option<ordin_types::WorkspaceId> {
    match place {
        MemoryPlace::Workspace(w) => Some(w),
        MemoryPlace::Scope(_) => None,
    }
}

impl Inner {
    /// MEM-TOP-2: a topic passes the personal-data scan (any span refuses) and the instruction
    /// check, as a new word does (MEM-TR-11).
    // spec: ordin-desktop/components/MemoryService#MEM-TOP-2
    pub(crate) async fn topic_is_clean(&self, topic: &str) -> Result<bool, MemoryError> {
        let screened = json!({ "topic": topic });
        let spans = memory::providers::LocalPiiDetector::new()
            .scan(&screened)
            .await
            .map_err(|_| MemoryError::internal())?;
        if !spans.is_empty() {
            return Ok(false);
        }
        Ok(!memory::write_pipeline::instruction_likelihood(&screened).is_instruction_like)
    }

    /// MEM-TOP-5: the spelling kept for `topic`: an existing topic of the place that reads the same
    /// (case and spacing aside) is reused, else `topic` trimmed.
    async fn topic_spelling(&self, place: MemoryPlace, topic: &str) -> Result<String, MemoryError> {
        let norm = memory::text::normalise_name(topic);
        let row = self
            .db
            .first(
                q("SELECT topic FROM memory_thing_topics WHERE place_key = $1 AND topic_norm = $2 \
                   ORDER BY set_at LIMIT 1")
                .text(store::scope_key(place))
                .text(norm),
            )
            .await?;
        match row {
            Some(row) => row.text("topic"),
            None => Ok(topic.trim().to_owned()),
        }
    }

    /// Writes a checked topic. `replace`: the person's set replaces a topic already there; Ordin's
    /// and an agent's never do (MEM-TOP-3).
    async fn put_topic(
        &self,
        place: MemoryPlace,
        id: &MemoryThingId,
        topic: &str,
        set_by: TopicSetBy,
        replace: bool,
    ) -> Result<bool, MemoryError> {
        let topic = self.topic_spelling(place, topic).await?;
        let conflict = if replace {
            "ON CONFLICT (place_key, thing_id) DO UPDATE SET topic = excluded.topic, \
             topic_norm = excluded.topic_norm, set_by = excluded.set_by, set_at = excluded.set_at"
        } else {
            "ON CONFLICT DO NOTHING"
        };
        let written = self
            .db
            .exec(
                q(format!(
                    "INSERT INTO memory_thing_topics (place_key, thing_id, topic, topic_norm, \
                     set_by, set_at) VALUES ($1, $2, $3, $4, $5, $6) {conflict}"
                ))
                .text(store::scope_key(place))
                .text(id.0.clone())
                .text(topic.clone())
                .text(memory::text::normalise_name(&topic))
                .text(set_by.as_str())
                .ts(Timestamp::now()),
            )
            .await?;
        Ok(written > 0)
    }

    /// MEM-TOP-6: a forgotten thing's topic goes with it.
    pub(crate) async fn drop_topic(
        &self,
        place: MemoryPlace,
        id: &MemoryThingId,
    ) -> Result<(), MemoryError> {
        self.db
            .exec(
                q("DELETE FROM memory_thing_topics WHERE place_key = $1 AND thing_id = $2")
                    .text(store::scope_key(place))
                    .text(id.0.clone()),
            )
            .await
            .map(|_| ())
    }

    /// The id of the place's thing named `name` (its name or one of its other names, case and
    /// spacing aside); None when the memory component has none.
    async fn thing_named(
        &self,
        place: MemoryPlace,
        name: &str,
    ) -> Result<Option<MemoryThingId>, MemoryError> {
        let norm = memory::text::normalise_name(name);
        let guard = self.auth.mint(self.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        let page = self
            .service
            .list_entities(
                guard.cx(&correlation_id, None),
                ListEntitiesRequest {
                    search: Some(name.trim().to_owned()),
                    cursor: None,
                },
            )
            .await
            .map_err(map_call_error)?
            .data;
        Ok(page
            .items
            .into_iter()
            .find(|item| {
                memory::text::normalise_name(&item.entity.canonical_name) == norm
                    || item
                        .entity
                        .aliases
                        .iter()
                        .any(|a| memory::text::normalise_name(a) == norm)
            })
            .map(|item| MemoryThingId(item.entity.id)))
    }

    /// MEM-TOP-3: after a fact is written, the topic its subject gave waits, by the subject's
    /// normalised name, until the memory component has written the thing (its job runs after the
    /// write returns). The first topic given for a name stands. A failure here never undoes the
    /// fact: it is logged.
    // spec: ordin-desktop/components/MemoryService#MEM-TOP-3
    pub(crate) async fn give_topic(
        &self,
        place: MemoryPlace,
        subject: &str,
        topic: &str,
        set_by: TopicSetBy,
    ) {
        let written = self
            .db
            .exec(
                q("INSERT INTO memory_topic_hints (place_key, subject_norm, topic, set_by, set_at) \
                   VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING")
                .text(store::scope_key(place))
                .text(memory::text::normalise_name(subject))
                .text(topic.trim().to_owned())
                .text(set_by.as_str())
                .ts(Timestamp::now()),
            )
            .await;
        match written {
            Ok(n) if n > 0 => {
                tracing::info!(target: "ordin::memory", set_by = set_by.as_str(), "memory.topic_given");
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(target: "ordin::memory", code = e.code(), "memory.topic_not_given");
            }
        }
    }

    /// MEM-TOP-3: the place's waiting topics whose thing now exists move to `memory_thing_topics`
    /// (a thing that has a topic keeps it); at most `HINTS_PER_READ` a read, oldest first. A hint
    /// older than `HINT_DAYS` whose thing never appeared is dropped.
    // spec: ordin-desktop/components/MemoryService#MEM-TOP-3
    async fn settle_hints(&self, place: MemoryPlace) -> Result<(), MemoryError> {
        let place_key = store::scope_key(place);
        let rows = self
            .db
            .all(
                q(
                    "SELECT subject_norm, topic, set_by, set_at FROM memory_topic_hints \
                   WHERE place_key = $1 ORDER BY set_at LIMIT $2",
                )
                .text(place_key.clone())
                .int(HINTS_PER_READ),
            )
            .await?;
        let stale = Timestamp(Timestamp::now().0 - chrono::Duration::days(HINT_DAYS));
        for row in &rows {
            let subject_norm = row.text("subject_norm")?;
            let set_by =
                TopicSetBy::parse(&row.text("set_by")?).ok_or_else(MemoryError::internal)?;
            let done = match self.thing_named(place, &subject_norm).await? {
                Some(id) => {
                    self.put_topic(place, &id, &row.text("topic")?, set_by, false)
                        .await?;
                    true
                }
                None => row.ts("set_at")?.0 < stale.0,
            };
            if done {
                self.db
                    .exec(
                        q("DELETE FROM memory_topic_hints WHERE place_key = $1 AND subject_norm = $2")
                            .text(place_key.clone())
                            .text(subject_norm),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

impl MemoryService {
    /// MEM-TOP-4: the place's topics.
    // spec: ordin-desktop/components/MemoryService#MEM-TOP-4
    pub async fn thing_topics(&self, place: MemoryPlace) -> Result<ThingTopicsView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.check_place(place)?;
        inner.settle_hints(place).await?;
        let rows = inner
            .db
            .all(
                q(
                    "SELECT thing_id, topic, set_by, set_at FROM memory_thing_topics \
                   WHERE place_key = $1 ORDER BY thing_id LIMIT $2",
                )
                .text(store::scope_key(place))
                .int(THINGS_MAX),
            )
            .await?;
        let mut things = Vec::with_capacity(rows.len());
        for row in &rows {
            things.push(ThingTopic {
                thing_id: MemoryThingId(row.text("thing_id")?),
                topic: row.text("topic")?,
                set_by: TopicSetBy::parse(&row.text("set_by")?)
                    .ok_or_else(MemoryError::internal)?,
                set_at: row.ts("set_at")?,
            });
        }
        let mut topics: Vec<String> = Vec::new();
        for thing in &things {
            let norm = memory::text::normalise_name(&thing.topic);
            if !topics
                .iter()
                .any(|t| memory::text::normalise_name(t) == norm)
            {
                topics.push(thing.topic.clone());
            }
        }
        topics.sort_by_key(|t| t.to_lowercase());
        topics.truncate(TOPICS_MAX);
        Ok(ThingTopicsView { things, topics })
    }

    /// MEM-TOP-5: the person sets, changes or (with `None`) clears a thing's topic.
    // spec: ordin-desktop/components/MemoryService#MEM-TOP-5
    pub async fn set_thing_topic(
        &self,
        place: MemoryPlace,
        id: MemoryThingId,
        topic: Option<String>,
    ) -> Result<ThingTopicsView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.check_place(place)?;
        if let Some(topic) = &topic {
            if !topic_ok(topic) {
                return Err(MemoryError::invalid("MEM_TOPIC_INVALID", "topic"));
            }
        }
        inner.detail(place, &id).await?;
        match &topic {
            Some(topic) => {
                if !inner.topic_is_clean(topic).await? {
                    return Err(MemoryError::Refused {
                        code: "MEM_TOPIC_REFUSED",
                    });
                }
                inner
                    .put_topic(place, &id, topic, TopicSetBy::Person, true)
                    .await?;
            }
            None => inner.drop_topic(place, &id).await?,
        }
        inner
            .journal(
                "topic_set",
                None,
                None,
                workspace_of(place),
                "The person set a thing's topic.",
                json!({ "place": store::scope_key(place), "thing_id": id.0, "cleared": topic.is_none() }),
            )
            .await;
        tracing::info!(target: "ordin::memory", cleared = topic.is_none(), "memory.topic_set");
        self.thing_topics(place).await
    }
}
