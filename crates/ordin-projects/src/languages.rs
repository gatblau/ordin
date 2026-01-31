//! Language plug-ins (ADR-024, ADR-055; Internal Logic 9.1, 9.2).

use ordin_types::domain::{LanguagePlugin, LanguagePluginSource, LanguagePluginStatus};
use ordin_types::LanguagePluginId;
use serde_json::json;

use crate::error::ProjectError;
use crate::lsp::InstanceKey;
use crate::service::ProjectService;
use crate::store;
use crate::types::{ProjectSettings, ServerPin};

/// The server a plug-in runs: the chosen one, else the default.
pub(crate) fn server_of(plugin: &LanguagePlugin) -> &str {
    plugin
        .chosen_server
        .as_deref()
        .unwrap_or(&plugin.default_server)
}

/// The pin of the plug-in's server in the current settings.
pub(crate) fn pin_of(settings: &ProjectSettings, plugin: &LanguagePlugin) -> Option<ServerPin> {
    let wanted = server_of(plugin);
    settings
        .language_servers
        .get(&plugin.language)?
        .approved
        .iter()
        .find(|pin| pin.name == wanted)
        .cloned()
}

impl ProjectService {
    /// Internal Logic 9.1.
    // spec: ordin-desktop/components/ProjectService#SA-CLOSE-02
    // spec: ordin-desktop/components/ProjectService#ADR-055
    pub(crate) async fn reconcile_languages(&self) -> Result<(), ProjectError> {
        let settings = self.current_settings();
        let plugins = store::plugins(&self.pool).await?;
        for (language, servers) in &settings.language_servers {
            let names: Vec<String> = servers.approved.iter().map(|p| p.name.clone()).collect();
            match plugins.iter().find(|p| &p.language == language) {
                Some(found) => {
                    let mut plugin = found.clone();
                    plugin.approved_servers = names.clone();
                    plugin.default_server = servers.default.clone();
                    if plugin
                        .chosen_server
                        .as_ref()
                        .is_some_and(|chosen| !names.contains(chosen))
                    {
                        plugin.chosen_server = None;
                    }
                    if plugin.status == LanguagePluginStatus::Removed {
                        plugin.status = LanguagePluginStatus::Available;
                    }
                    plugin.pinned_version = pin_of(&settings, &plugin).map(|pin| pin.version);
                    store::update_plugin(&self.pool, &plugin).await?;
                }
                None => {
                    let mut plugin = LanguagePlugin {
                        id: LanguagePluginId::new(),
                        language: language.clone(),
                        approved_servers: names,
                        default_server: servers.default.clone(),
                        chosen_server: None,
                        pinned_version: None,
                        source: LanguagePluginSource::OrganisationProfile,
                        enabled: true,
                        server_settings: json!({}),
                        status: LanguagePluginStatus::Available,
                    };
                    plugin.pinned_version = pin_of(&settings, &plugin).map(|pin| pin.version);
                    store::insert_plugin(&self.pool, &plugin).await?;
                }
            }
        }
        for plugin in plugins {
            if !settings.language_servers.contains_key(&plugin.language) {
                let mut withdrawn = plugin;
                withdrawn.status = LanguagePluginStatus::Removed;
                store::update_plugin(&self.pool, &withdrawn).await?;
            }
        }
        Ok(())
    }

    pub async fn language_plugins(&self) -> Result<Vec<LanguagePlugin>, ProjectError> {
        store::plugins(&self.pool).await
    }

    /// Internal Logic 9.2. Invalid JSON is refused and nothing is saved (ADR-055).
    // spec: ordin-desktop/components/ProjectService#ADR-055
    pub async fn configure_language(
        &self,
        language: &str,
        chosen_server: Option<String>,
        enabled: bool,
        server_settings: serde_json::Value,
    ) -> Result<LanguagePlugin, ProjectError> {
        let size = serde_json::to_vec(&server_settings)
            .map_err(|_| ProjectError::Invalid("LANG_SETTINGS_INVALID"))?
            .len();
        if !server_settings.is_object() || size > crate::MAX_SERVER_SETTINGS_BYTES {
            return Err(ProjectError::Invalid("LANG_SETTINGS_INVALID"));
        }
        // A language the profile never offered, or withdrew, has no approved server.
        let mut plugin = store::plugin(&self.pool, language)
            .await?
            .filter(|p| p.status != LanguagePluginStatus::Removed)
            .ok_or(ProjectError::Invalid("LANG_SERVER_NOT_APPROVED"))?;
        if chosen_server
            .as_ref()
            .is_some_and(|chosen| !plugin.approved_servers.contains(chosen))
        {
            return Err(ProjectError::Invalid("LANG_SERVER_NOT_APPROVED"));
        }
        plugin.chosen_server = chosen_server;
        plugin.enabled = enabled;
        plugin.server_settings = server_settings.clone();
        plugin.pinned_version = pin_of(&self.current_settings(), &plugin).map(|pin| pin.version);
        if plugin.status == LanguagePluginStatus::Available {
            plugin.status = LanguagePluginStatus::Added;
        }
        store::update_plugin(&self.pool, &plugin).await?;
        if enabled {
            self.configure_running(language, &server_settings);
        } else {
            self.stop_language_instances(language).await;
        }
        self.record(
            "language_configured",
            None,
            None,
            "The engineer configured a language server.",
            json!({ "language": language, "enabled": enabled }),
        )
        .await;
        Ok(plugin)
    }

    /// Stops the language's editor instances and starts them again. A failure to install is
    /// `PROJ_SERVER_INSTALL_FAILED`; the next file opened tries again.
    pub async fn restart_language_server(&self, language: &str) -> Result<(), ProjectError> {
        let running = self.stop_language_instances(language).await;
        for (key, root) in running {
            if matches!(key, InstanceKey::Editor(..)) {
                self.instance(key, &root, language).await?;
            }
        }
        Ok(())
    }
}
