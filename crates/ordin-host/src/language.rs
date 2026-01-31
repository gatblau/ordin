//! Ordin's language and the person's language drafts (section L, ADR-109): the tool `change_setting`
//! (OH-LG-3), the offer to draft a missing language (OH-LG-4) and the drafting job (OH-LG-5).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ordin_mcp_front::{ToolFailure, ToolOutput};
use ordin_types::common::SetBy;
use ordin_types::time::Timestamp;
use ordin_types::WorkspaceId;
use ordin_usage_planner::{EstimateRequest, WorkShape};

use crate::error::OhError;
use crate::host::{lock, Kept, OrdinHost, Turn, COMPONENT};
use crate::ids::ConversationId;
use crate::ports::{DraftEnding, LocMessage, Match, Source};
use crate::store;
use crate::tools::fail;
use crate::types::ChangeSettingArgs;

/// OH-LG-5c: at most 100 messages and 24,000 characters of en-GB source a batch.
const BATCH_MESSAGES: usize = 100;
const BATCH_CHARS: usize = 24_000;

/// The one drafting job of the installation (in memory; a restart ends it).
pub(crate) struct DraftJob {
    pub locale: String,
    pub stop: Arc<AtomicBool>,
}

/// The batches of step 54.3, in message-id order.
pub(crate) fn batches(mut sources: Vec<Source>) -> Vec<Vec<Source>> {
    sources.sort_by(|a, b| a.message_id.cmp(&b.message_id));
    let mut out: Vec<Vec<Source>> = Vec::new();
    let mut chars = 0;
    for s in sources {
        let n = s.english.chars().count();
        let fits = out
            .last()
            .is_some_and(|b| b.len() < BATCH_MESSAGES && chars + n <= BATCH_CHARS);
        if !fits {
            out.push(Vec::new());
            chars = 0;
        }
        chars += n;
        if let Some(b) = out.last_mut() {
            b.push(s);
        }
    }
    out
}

/// The template text of `language-draft` with the tag and name placed by fixed code (step 42.2).
pub(crate) fn draft_template(tag: &str, name: &str) -> String {
    format!(
        "Translate each Fluent message in the data below from en-GB into {tag} ({name}). Keep every message identifier, \
         every {{ $variable }} and every function call exactly as they are. Use the plural categories of {tag}. Keep the \
         meaning, in plain, brief words. Reply with the Fluent messages only, with no other text."
    )
}

fn name_ok(name: &str) -> Option<String> {
    let t = name.trim();
    (!t.is_empty() && t.chars().count() <= 60).then(|| t.to_owned())
}

impl OrdinHost {
    /// Step 52 (`change_setting`).
    // spec: ordin-desktop/components/OrdinHost#OH-LG-3
    pub(crate) async fn change_setting(
        &self,
        ws: WorkspaceId,
        args: ChangeSettingArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        // OH-LG-3a: only a turn started by the person's own message changes a setting; a Slack
        // turn, a crew turn, a fixed turn of this crate and Ordin's work on its seed do not.
        let persons = lock(&self.state)
            .convs
            .get(&ws)
            .is_some_and(|c| c.persons_turn && c.slack.is_none());
        if !persons {
            return Err(fail(
                "OH_NOT_PERSONS_WORDS",
                "Settings change only when the person asks.",
                false,
            ));
        }
        let out = |t: String| ToolOutput {
            text: t,
            structured: None,
        };
        let failed = |e: OhError| fail(&e.code(), "That did not work; try again.", e.retryable());
        let (setting, interface) = match &args {
            ChangeSettingArgs::Tone { .. } => ("tone", None),
            ChangeSettingArgs::Register { .. } => ("register", None),
            ChangeSettingArgs::Length { .. } => ("length", None),
            ChangeSettingArgs::NameToUse { .. } => ("name_to_use", None),
            ChangeSettingArgs::InterfaceLanguage { locale, name } => {
                ("interface_language", Some((locale.clone(), name.clone())))
            }
        };
        if let Some((locale, name)) = interface {
            return self.change_language(ws, &locale, &name).await;
        }
        // OH-LG-3b: step 36's rules for that one field.
        let mut v = self.voice_settings().await.map_err(failed)?;
        let fixed = || fail("VOICE_FIXED_BY_COMPANY", "Your company sets this.", false);
        match args {
            ChangeSettingArgs::Tone { value } => {
                if v.tone.set_by == SetBy::Company {
                    return Err(fixed());
                }
                v.tone.value = value;
            }
            ChangeSettingArgs::Register { value } => {
                if v.register.set_by == SetBy::Company {
                    return Err(fixed());
                }
                v.register.value = value;
            }
            ChangeSettingArgs::Length { value } => {
                if v.length.set_by == SetBy::Company {
                    return Err(fixed());
                }
                v.length.value = value;
            }
            ChangeSettingArgs::NameToUse { value } => {
                if v.name_to_use.set_by == SetBy::Company {
                    return Err(fixed());
                }
                let value = value.filter(|n| !n.is_empty());
                if value.as_ref().is_some_and(|n| n.chars().count() > 60) {
                    return Err(fail(
                        "VOICE_NAME_TOO_LONG",
                        "Use a name of at most 60 characters.",
                        false,
                    ));
                }
                v.name_to_use.value = value;
            }
            ChangeSettingArgs::InterfaceLanguage { .. } => {}
        }
        v.updated_at = Timestamp::now();
        {
            let mut c = self.pool.acquire().await.map_err(|e| failed(e.into()))?;
            store::put_voice(&mut c, &v)
                .await
                .map_err(|e| failed(e.into()))?;
        }
        {
            let mut state = lock(&self.state);
            for (w, conv) in state.convs.iter_mut() {
                if *w == ws {
                    conv.voice_changed_by_ordin = true;
                } else {
                    conv.voice_changed = true;
                }
            }
        }
        tracing::info!(
            component = COMPONENT,
            setting,
            by = "words",
            "ordin_host.setting_changed"
        );
        Ok(out("Changed. Say so in one line in the new style.".into()))
    }

    /// OH-LG-3c.
    async fn change_language(
        &self,
        ws: WorkspaceId,
        locale: &str,
        name: &str,
    ) -> Result<ToolOutput, ToolFailure> {
        let out = |t: String| ToolOutput {
            text: t,
            structured: None,
        };
        let refused = |e: crate::ports::DepError| {
            let message = if e.message.is_empty() {
                e.code.clone()
            } else {
                e.message.clone()
            };
            fail(&e.code, &message, e.retryable)
        };
        let matched = self
            .ports
            .localisation
            .match_language(locale)
            .map_err(refused)?;
        match matched {
            Match::Choosable(tag) => {
                let active = self.ports.localisation.set_locale(&tag).await.map_err(refused)?;
                if let Some(conv) = lock(&self.state).convs.get_mut(&ws) {
                    conv.stated_locale = Some(active.clone());
                }
                tracing::info!(component = COMPONENT, setting = "interface_language", by = "words", "ordin_host.setting_changed");
                Ok(out(format!(
                    "The interface language is now {active}. Write in {active} from now on, and confirm in one line."
                )))
            }
            Match::NotConfirmed(tag) => Ok(out(format!(
                "{tag} exists but the person has not reviewed it yet. Tell them, in one line, to review it in Settings › Language."
            ))),
            Match::NotOffered(_) => Err(fail("LOC_NOT_OFFERED", "The company does not offer that language.", false)),
            Match::Missing(tag) => self.offer_draft_language(ws, &tag, name).await,
        }
    }

    /// OH-LG-4: the offer; nothing is drafted before the answer.
    // spec: ordin-desktop/components/OrdinHost#OH-LG-4
    async fn offer_draft_language(
        &self,
        ws: WorkspaceId,
        tag: &str,
        name: &str,
    ) -> Result<ToolOutput, ToolFailure> {
        let name = name_ok(name).ok_or_else(|| {
            fail(
                "OH_INPUT_INVALID",
                "Give the language's name in 1 to 60 characters.",
                false,
            )
        })?;
        let sources = self
            .ports
            .localisation
            .draft_sources(tag)
            .map_err(|e| fail(&e.code, &e.message, e.retryable))?;
        let bytes: u64 = sources.iter().map(|s| s.english.len() as u64).sum();
        let count = u32::try_from(batches(sources).len()).unwrap_or(u32::MAX);
        let estimate = self
            .ports
            .usage
            .estimate(EstimateRequest {
                work_key: format!("language:{tag}"),
                shape: WorkShape::Task {
                    documents: count,
                    total_bytes: bytes,
                },
            })
            .await;
        let question = match estimate {
            Ok(e) => self.ports.localisation.message(&LocMessage::LanguageOffer {
                name: name.clone(),
                band: crate::store::word(&e.band),
                min: e.min_turns,
                max: e.max_turns,
            }),
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.estimate_unavailable"
                );
                self.ports
                    .localisation
                    .message(&LocMessage::LanguageOfferNoEstimate { name: name.clone() })
            }
        };
        let choices = vec![
            self.ports.localisation.message(&LocMessage::OfferDraft),
            self.ports.localisation.message(&LocMessage::OfferNotNow),
        ];
        self.ask(
            ws,
            &question,
            choices,
            Some(Kept::LanguageOffer {
                tag: tag.to_owned(),
                name,
            }),
        )
        .await
        .map_err(|e| fail(&e.code(), "That did not work; try again.", e.retryable()))?;
        Ok(ToolOutput {
            text: format!("No {tag} wording exists. The person was asked whether to draft it; nothing is drafted unless they accept."),
            structured: None,
        })
    }

    /// OH-LG-4.4: the answer to the offer.
    pub(crate) async fn language_offer_answered(
        &self,
        ws: WorkspaceId,
        tag: &str,
        name: &str,
        answer: &str,
    ) -> Result<(), OhError> {
        let draft = self.ports.localisation.message(&LocMessage::OfferDraft);
        if answer != draft {
            self.send_turn(ws, Turn::person(answer.to_owned())).await?;
            return Ok(());
        }
        let from = self.conv_run(ws).map(|(c, _)| (ws, c));
        let text = match self.draft_language_from(tag, name, from).await {
            Ok(_) => format!("Drafting {tag} has started. The person reviews it in Settings › Language before it can be used."),
            Err(e) => format!("Drafting {tag} could not start ({}).", e.code()),
        };
        self.send_turn(ws, Turn::fixed(text)).await?;
        Ok(())
    }

    /// `ordin.estimate_language_draft` (OH-LG-5a).
    pub async fn estimate_language_draft(
        &self,
        locale: String,
    ) -> Result<(u32, Option<ordin_service_api::blocks::UsageEstimate>), OhError> {
        let sources = self
            .ports
            .localisation
            .draft_sources(&locale)
            .map_err(|e| OhError::dependency("localisation", &e))?;
        let n = u32::try_from(sources.len()).unwrap_or(u32::MAX);
        let bytes: u64 = sources.iter().map(|s| s.english.len() as u64).sum();
        let count = u32::try_from(batches(sources).len()).unwrap_or(u32::MAX);
        let estimate = match self
            .ports
            .usage
            .estimate(EstimateRequest {
                work_key: format!("language:{locale}"),
                shape: WorkShape::Task {
                    documents: count,
                    total_bytes: bytes,
                },
            })
            .await
        {
            Ok(e) => serde_json::to_value(e)
                .ok()
                .and_then(|v| serde_json::from_value(v).ok()),
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.estimate_unavailable"
                );
                None
            }
        };
        Ok((n, estimate))
    }

    /// `ordin.draft_language` (OH-LG-5b). Returns the number of messages to draft.
    pub async fn draft_language(&self, locale: String, name: String) -> Result<u32, OhError> {
        self.draft_language_from(&locale, &name, None).await
    }

    async fn draft_language_from(
        &self,
        locale: &str,
        name: &str,
        from: Option<(WorkspaceId, ConversationId)>,
    ) -> Result<u32, OhError> {
        let name = name_ok(name).ok_or_else(|| OhError::input("OH_INPUT_INVALID", "name"))?;
        if lock(&self.state).draft.is_some() {
            return Err(OhError::DraftRunning);
        }
        self.ports
            .usage
            .may_start_ordin_work()
            .await
            .map_err(|e| OhError::DependencyFailed {
                dependency: "usage",
                code: e.code,
                retryable: true,
            })?;
        let sources = self
            .ports
            .localisation
            .draft_sources(locale)
            .map_err(|e| OhError::dependency("localisation", &e))?;
        if sources.is_empty() {
            return Err(OhError::NothingToDraft);
        }
        let n = u32::try_from(sources.len()).unwrap_or(u32::MAX);
        self.ports
            .localisation
            .begin_draft(locale, &name, n)
            .await
            .map_err(|e| OhError::dependency("localisation", &e))?;
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut state = lock(&self.state);
            if state.draft.is_some() {
                return Err(OhError::DraftRunning);
            }
            state.draft = Some(DraftJob {
                locale: locale.to_owned(),
                stop: stop.clone(),
            });
        }
        let parts = batches(sources);
        tracing::info!(
            component = COMPONENT,
            locale,
            messages = n,
            batches = parts.len(),
            "ordin_host.language_draft_started"
        );
        let me = self.me.clone();
        let locale = locale.to_owned();
        tokio::spawn(async move {
            if let Some(host) = me.upgrade() {
                host.draft_job(locale, name, parts, stop, from).await;
            }
        });
        Ok(n)
    }

    /// `ordin.stop_language_draft` (OH-LG-5d).
    pub fn stop_language_draft(&self, locale: String) {
        if let Some(job) = lock(&self.state).draft.as_ref() {
            if job.locale == locale {
                job.stop.store(true, Ordering::SeqCst);
            }
        }
    }

    /// OH-LG-5c and 5e.
    // spec: ordin-desktop/components/OrdinHost#OH-LG-5
    async fn draft_job(
        &self,
        locale: String,
        name: String,
        parts: Vec<Vec<Source>>,
        stop: Arc<AtomicBool>,
        from: Option<(WorkspaceId, ConversationId)>,
    ) {
        let mut stored = 0u32;
        let mut left_out = 0u32;
        let mut end = DraftEnding::Finished;
        let template = draft_template(&locale, &name);
        for (i, batch) in parts.iter().enumerate() {
            if self.ports.usage.may_start_ordin_work().await.is_err() {
                end = DraftEnding::Paused;
                break;
            }
            if stop.load(Ordering::SeqCst) {
                end = DraftEnding::Stopped;
                break;
            }
            let data: Vec<String> = batch.iter().map(|s| s.english.clone()).collect();
            let data = serde_json::Value::String(data.join("\n\n"));
            let mut reply = None;
            let mut failed = None;
            for _ in 0..2 {
                match self
                    .generate_text(&template, &data, from.map(|f| f.0))
                    .await
                {
                    Ok(text) => {
                        reply = Some(text);
                        break;
                    }
                    Err(code) => failed = Some(code),
                }
            }
            // Step 54.3.4: logged once, when the batch is left out after its second failure.
            if let (None, Some(code)) = (&reply, &failed) {
                tracing::warn!(
                    component = COMPONENT,
                    locale = locale.as_str(),
                    batch = i,
                    code = code.as_str(),
                    "ordin_host.language_draft_batch_failed"
                );
            }
            let batch_len = u32::try_from(batch.len()).unwrap_or(u32::MAX);
            match reply {
                Some(text) => match self.ports.localisation.add_drafted(&locale, &text).await {
                    Ok((s, l)) => {
                        stored += s;
                        left_out += l;
                    }
                    Err(e) => {
                        tracing::warn!(
                            component = COMPONENT,
                            code = e.code.as_str(),
                            "ordin_host.language_draft_batch_failed"
                        );
                        left_out += batch_len;
                    }
                },
                None => left_out += batch_len,
            }
        }
        let english = self
            .ports
            .localisation
            .end_draft(&locale, end)
            .await
            .unwrap_or(0);
        lock(&self.state).draft = None;
        tracing::info!(
            component = COMPONENT,
            locale = locale.to_string().as_str(),
            end = match end {
                DraftEnding::Finished => "finished",
                DraftEnding::Stopped => "stopped",
                DraftEnding::Paused => "paused",
            },
            stored,
            left_out,
            "ordin_host.language_draft_ended"
        );
        if let (DraftEnding::Finished, Some((ws, conv))) = (end, from) {
            if self.conv_run(ws).map(|c| c.0) == Some(conv) {
                let text = self.ports.localisation.message(&LocMessage::Drafted {
                    name,
                    stored,
                    english,
                });
                let _ = self.show_message(ws, &text).await;
            }
        }
    }
}
