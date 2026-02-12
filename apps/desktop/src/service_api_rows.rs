// The delegations of `DesktopServiceApi`, one arm per catalogue row: the owner's method of the
// same verb, or the one the row's *Note* names (ServiceApi.md *Command catalogue*), with each
// input field passed in the owner's serde form and the result restated. Generated from the
// catalogue rows and the owners' signatures, then kept by hand. A row with no arm here answers
// `SVC_NOT_IN_THIS_VERSION` (see the SPEC-GAP markers of `service_api.rs`).
macro_rules! delegate {
    ($api:expr, $ctx:expr, $input:expr, home_opened) => {{
        // home.opened: ordin-home-search `home_opened`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.home)?;
        let _ = $input;
        let out = m.home_opened().await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, home_workspace_opened) => {{
        // home.workspace_opened: ordin-home-search `workspace_opened`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.home)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.workspace_opened(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, home_changed_marks) => {{
        // home.changed_marks: ordin-home-search `changed_marks`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.home)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.changed_marks(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, home_document_opened) => {{
        // home.document_opened: ordin-home-search `document_opened`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.home)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.item)?;
        let out = m.document_opened(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, home_run_again_inputs) => {{
        // home.run_again_inputs: ordin-home-search `run_again_inputs`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.home)?;
        let input = $input;
        let p0 = $api.restate(input.subject)?;
        let out = m.run_again_inputs(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, home_needs_you) => {{
        // home.needs_you: ordin-home-search `needs_you`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.home)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.needs_you(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_create) => {{
        // workspaces.create: ordin-workspaces `create`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.name)?;
        let out = m.create(p0).await.map_err(|e| $api.module_error(e))?;
        // The starter agents join the new workspace in a task; the reply does not wait for it.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-DOC-1
        if let (Some(packages), Some(localisation)) = (
            $api.modules.packages.clone(),
            $api.modules.localisation.clone(),
        ) {
            crate::bundled::spawn_starter_agents(packages, localisation, out.id);
        }
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_rename) => {{
        // workspaces.rename: ordin-workspaces `rename`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.name)?;
        let out = m.rename(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_archive) => {{
        // workspaces.archive: ordin-workspaces `archive`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.archive(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_restore) => {{
        // workspaces.restore: ordin-workspaces `restore`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.restore(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_bin_impact) => {{
        // workspaces.bin_impact: ordin-workspaces `bin_impact`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.bin_impact(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_move_to_bin) => {{
        // workspaces.move_to_bin: ordin-workspaces `move_to_bin`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.move_to_bin(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_restore_from_bin) => {{
        // workspaces.restore_from_bin: ordin-workspaces `restore_from_bin`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.restore_from_bin(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_delete_from_bin) => {{
        // workspaces.delete_from_bin: ordin-workspaces `delete_from_bin`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let removed = p0;
        let out = m.delete_from_bin(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-1
        $api.minutes_workspace_removed(removed).await;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_empty_bin) => {{
        // workspaces.empty_bin: ordin-workspaces `empty_bin`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let _ = $input;
        // The binned workspaces, so their minutes go with them (DSK-MIN-1).
        let binned = m
            .list(::ordin_workspaces::WorkspaceListFilter::InBin)
            .await
            .map_err(|e| $api.module_error(e))?;
        let out = m.empty_bin().await.map_err(|e| $api.module_error(e))?;
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-1
        for workspace in binned {
            $api.minutes_workspace_removed(workspace.id).await;
        }
        let mut obj = ::serde_json::Map::new();
        obj.insert("deleted".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_list) => {{
        // workspaces.list: ordin-workspaces `list`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.filter)?;
        let out = m.list(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_open) => {{
        // workspaces.open: ordin-workspaces `open`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.open(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_add_link) => {{
        // workspaces.add_link: ordin-workspaces `add_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.location)?;
        let p2 = $api.restate(input.display_name)?;
        let out = m.add_link(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_confirm_link) => {{
        // workspaces.confirm_link: ordin-workspaces `confirm_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.source_link_id)?;
        let out = m.confirm_link(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_decline_link) => {{
        // workspaces.decline_link: ordin-workspaces `decline_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.source_link_id)?;
        let out = m.decline_link(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_remove_link) => {{
        // workspaces.remove_link: ordin-workspaces `remove_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.source_link_id)?;
        let out = m.remove_link(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_link_progress) => {{
        // workspaces.link_progress: ordin-workspaces `link_progress`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.source_link_id)?;
        let out = m.link_progress(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_set_grant) => {{
        // workspaces.set_grant: ordin-workspaces `set_grant`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.grantee)?;
        let p2 = $api.restate(input.place_id)?;
        let p3 = $api.restate(input.access)?;
        let out = m.set_grant(p0, p1, p2, p3).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_grants) => {{
        // workspaces.grants: ordin-workspaces `grants`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.grants(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_propose_confluence_link) => {{
        // workspaces.propose_confluence_link: ordin-workspaces `propose_confluence_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.place_id)?;
        let p2 = $api.restate(input.folder)?;
        let p3 = $api.restate(input.location)?;
        let out = m.propose_confluence_link(p0, p1, p2, p3).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_pause_confluence_link) => {{
        // workspaces.pause_confluence_link: ordin-workspaces `pause_confluence_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.confluence_link_id)?;
        let out = m.pause_confluence_link(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_resume_confluence_link) => {{
        // workspaces.resume_confluence_link: ordin-workspaces `resume_confluence_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.confluence_link_id)?;
        let out = m.resume_confluence_link(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_remove_confluence_link) => {{
        // workspaces.remove_confluence_link: ordin-workspaces `remove_confluence_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.confluence_link_id)?;
        let out = m.remove_confluence_link(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    // A linked issue's items and bringing one in (SVC-SCR-5, DSK-REC-40.5).
    ($api:expr, $ctx:expr, $input:expr, workspaces_working_copy_items) => {{
        // workspaces.working_copy_items: ordin-workspaces `working_copy_items`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let out = m
            .working_copy_items(input.source_link_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_read_item) => {{
        // workspaces.read_item: ordin-workspaces `read_item`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let out = m
            .read_item(input.source_link_id, &input.origin_path)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_confluence_links) => {{
        // workspaces.confluence_links: ordin-workspaces `confluence_links` (SVC-CL-1)
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.confluence_links(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_confluence_publishing) => {{
        // workspaces.confluence_publishing: ordin-workspaces `confluence_publishing` (SVC-AUTO-1)
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.confluence_link_id)?;
        let out = m.confluence_publishing(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_email_links) => {{
        // workspaces.email_links: ordin-workspaces `email_links`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.email_links(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_remove_email_link) => {{
        // workspaces.remove_email_link: ordin-workspaces `remove_email_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.email_link_id)?;
        let out = m.remove_email_link(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_set_email_grant) => {{
        // workspaces.set_email_grant: ordin-workspaces `set_email_grant`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.agent_instance_id)?;
        let p2 = $api.restate(input.access)?;
        let out = m.set_email_grant(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_set_telegram_grant) => {{
        // workspaces.set_telegram_grant: ordin-workspaces `set_telegram_grant`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.agent_instance_id)?;
        let p2 = $api.restate(input.access)?;
        let out = m.set_telegram_grant(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_telegram_grants) => {{
        // workspaces.telegram_grants: ordin-workspaces `telegram_grants`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.telegram_grants(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_set_slack_grant) => {{
        // workspaces.set_slack_grant: ordin-workspaces `set_slack_grant`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.agent_instance_id)?;
        let p2 = $api.restate(input.access)?;
        let out = m.set_slack_grant(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_slack_grants) => {{
        // workspaces.slack_grants: ordin-workspaces `slack_grants`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.slack_grants(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_rename_place) => {{
        // files.rename_place: ordin-files `rename_place`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let p0 = $api.restate(input.place_id)?;
        let p1 = $api.restate(input.friendly_name)?;
        let out = m.rename_place(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_list_folder) => {{
        // files.list_folder: ordin-files `list_folder`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let p0 = $api.restate(input.place_id)?;
        let p1: String = $api.restate(input.folder)?;
        let p2 = $api.restate(input.sort)?;
        let p3 = $api.restate(input.cursor)?;
        let out = m.list_folder(p0, &p1, p2, p3).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_preview) => {{
        // files.preview: ordin-files `preview`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let p0 = $api.restate(input.place_id)?;
        let p1: String = $api.restate(input.path)?;
        // DSK-CFP-1: a linked Confluence page reads as the Markdown it converts to.
        let out = if crate::confluence_preview::is_page(&p1) {
            let file = m.read_file(p0, &p1).await.map_err(|e| $api.module_error(e))?;
            crate::confluence_preview::preview(&file.bytes)
        } else {
            m.preview(p0, &p1).await.map_err(|e| $api.module_error(e))?
        };
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_versions) => {{
        // files.versions: ordin-files `versions`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let p0 = $api.restate(input.place_id)?;
        let p1: String = $api.restate(input.path)?;
        let out = m.versions(p0, &p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_restore_version) => {{
        // files.restore_version: ordin-files `restore_version`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let p0 = $api.restate(input.file_version_id)?;
        let out = m.restore_version(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_import) => {{
        // files.import: ordin-files `import_files`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let p0 = $api.restate(input.place_id)?;
        let p1: String = $api.restate(input.folder)?;
        let p2 = $api.restate(input.sources)?;
        let out = m.import_files(p0, &p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_move_to_bin) => {{
        // files.move_to_bin: ordin-files `move_to_bin`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let p0 = $api.restate(input.place_id)?;
        let p1: String = $api.restate(input.path)?;
        let out = m.move_to_bin(p0, &p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_restore_from_bin) => {{
        // files.restore_from_bin: ordin-files `restore_from_bin`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let p0 = $api.restate(input.ticket)?;
        let out = m.restore_from_bin(&p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_search) => {{
        // files.search: ordin-files `search`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let p0: Vec<_> = $api.restate(input.place_ids)?;
        let p1: String = $api.restate(input.query)?;
        let p2 = $api.restate(input.limit)?;
        let out = m.search(p0.as_slice(), &p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, craft_open_with_system_default) => {{
        // craft.open_with_system_default: ordin-craft-toolkit `open_with_system_default`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.craft)?;
        let input = $input;
        let p0 = $api.restate(input.place_id)?;
        let p1: String = $api.restate(input.path)?;
        let out = m.open_with_system_default(p0, &p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, craft_installations) => {{
        // craft.installations: ordin-craft-toolkit `installations`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.craft)?;
        let _ = $input;
        let out = m.installations().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, push_list_pending) => {{
        // push.list_pending: ordin-push `list_pending`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.push)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.list_pending(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, push_choose_version) => {{
        // push.choose_version: ordin-push `choose_version`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.push)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.choose_version(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, push_try_again) => {{
        // push.try_again: ordin-push `try_again`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.push)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.try_again(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, push_publish_copy) => {{
        // push.publish_copy: ordin-push `publish_copy`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.push)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.publish_copy(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, push_update_copies) => {{
        // push.update_copies: ordin-push `update_copies`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.push)?;
        let input = $input;
        let p0 = $api.restate(input.original)?;
        let p1 = $api.restate(input.workspace_id)?;
        let out = m.update_copies(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, push_copy_action) => {{
        // push.copy_action: ordin-push `copy_action`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.push)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.copy_action(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, confluence_ready_to_publish) => {{
        // confluence.ready_to_publish: ordin-confluence `ready_to_publish`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.confluence)?;
        let input = $input;
        let p0 = $api.restate(input.confluence_link_id)?;
        let out = m.ready_to_publish(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_send_message) => {{
        // ordin.send_message: ordin-host `send_message`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.text)?;
        // `attachments` is optional on the wire and the conversation screen leaves it out: absent
        // is no attachment, not a value the restatement refuses.
        let p2 = $api.restate(input.attachments.unwrap_or_default())?;
        // spec: ordin-desktop/components/ServiceApi#SVC-MENU-1
        let out = m
            .send_message_from(p0, p1, p2, input.from_menu)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_answer_block) => {{
        // ordin.answer_block: ordin-host `answer_block`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.block_id)?;
        let p2 = $api.restate(input.answer)?;
        let out = m.answer_block(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        let mut obj = ::serde_json::Map::new();
        obj.insert("accepted".to_owned(), ::serde_json::Value::Bool(true));
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_summarise) => {{
        // ordin.summarise: ordin-host `summarise`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.summarise(p0).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("conversation_id".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_start_fresh) => {{
        // ordin.start_fresh: ordin-host `start_fresh`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.kept_points)?;
        let out = m.start_fresh(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("conversation_id".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_continue_conversation) => {{
        // ordin.continue_conversation: ordin-host `continue_conversation`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.conversation_id)?;
        let p2 = $api.restate(input.kept_points)?;
        let out = m.continue_conversation(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("conversation_id".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_delete_conversation) => {{
        // ordin.delete_conversation: ordin-host `delete_conversation`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.conversation_id)?;
        m.delete_conversation(p0).await.map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_list_conversations) => {{
        // ordin.list_conversations: ordin-host `list_conversations`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.list_conversations(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_read_conversation) => {{
        // ordin.read_conversation: ordin-host `read_conversation`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.conversation_id)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.read_conversation(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_ask_about_conversation) => {{
        // ordin.ask_about_conversation: ordin-host `ask_about_conversation`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.conversation_id)?;
        let p2 = $api.restate(input.question)?;
        let out = m.ask_about_conversation(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        let mut obj = ::serde_json::Map::new();
        obj.insert("accepted".to_owned(), ::serde_json::Value::Bool(true));
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_block) => {{
        // ordin.block: ordin-host `block`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.block_id)?;
        let out = m.block(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_bring_page_up_to_date) => {{
        // ordin.bring_page_up_to_date: ordin-host `bring_page_up_to_date`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.page_id)?;
        let out = m.bring_page_up_to_date(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        let mut obj = ::serde_json::Map::new();
        obj.insert("accepted".to_owned(), ::serde_json::Value::Bool(true));
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_estimate_language_draft) => {{
        // ordin.estimate_language_draft: ordin-host `estimate_language_draft`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.locale)?;
        let out = m.estimate_language_draft(p0).await.map_err(|e| $api.module_error(e))?;
        let (o0, o1) = out;
        let mut obj = ::serde_json::Map::new();
        obj.insert("messages".to_owned(), $api.value(o0)?);
        obj.insert("estimate".to_owned(), $api.value(o1)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_stop_language_draft) => {{
        // ordin.stop_language_draft: ordin-host `stop_language_draft`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.locale)?;
        let out = m.stop_language_draft(p0);
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, voice_get) => {{
        // voice.get: ordin-host `voice_get`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let _ = $input;
        let out = m.voice_get().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, voice_update) => {{
        // voice.update: ordin-host `voice_update`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.voice_update(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, runs_get) => {{
        // runs.get: ordin-session-supervisor `get`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.supervisor)?;
        let input = $input;
        let p0 = $api.restate(input.run_id)?;
        let out = m.get(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, runs_try_again) => {{
        // runs.try_again: ordin-host `try_again_run` (OrdinHost.md OH-TA-1; ServiceApi.md SVC-TA-1)
        // spec: ordin-desktop/components/ServiceApi#SVC-TA-1
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.run_id)?;
        let out = m.try_again_run(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, runs_start_task) => {{
        // runs.start_task: ordin-host `start_task_from_form` (OrdinHost.md OH-TF-1; ServiceApi.md SVC-TF-1)
        // spec: ordin-desktop/components/ServiceApi#SVC-TF-1
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.start_task_from_form(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, runs_list) => {{
        // runs.list: ordin-session-supervisor `list`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.supervisor)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.list(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, crews_get) => {{
        // crews.get: ordin-crew-orchestrator `get`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.orchestrator)?;
        let input = $input;
        let p0 = $api.restate(input.crew_run_id)?;
        let out = m.get(p0).await.map_err(|e| $api.module_error(e))?;
        let (o0, o1) = out;
        let mut obj = ::serde_json::Map::new();
        obj.insert("crew_run".to_owned(), $api.value(o0)?);
        obj.insert("rounds".to_owned(), $api.value(o1)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, crews_discard_plan) => {{
        // crews.discard_plan: ordin-host `crews_discard_plan`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.crew_charter_id)?;
        let out = m.crews_discard_plan(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, dashboards_refresh) => {{
        // dashboards.refresh: ordin-host `dashboards_refresh`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.dashboard_id)?;
        let out = m.dashboards_refresh(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_list) => {{
        // workflows.list: ordin-host `workflows_list`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.workflows_list(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_get) => {{
        // workflows.get: ordin-host `workflows_get`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let p1 = $api.restate(input.version)?;
        let out = m.workflows_get(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_check) => {{
        // workflows.check: ordin-host `workflows_check`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.file)?;
        let out = m.workflows_check(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_save) => {{
        // workflows.save: ordin-host `workflows_save`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let p1 = $api.restate(input.base_version)?;
        let p2 = $api.restate(input.file)?;
        let p3 = $api.restate(input.change_note)?;
        let out = m.workflows_save(p0, p1, p2, p3).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("version".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_discard_draft) => {{
        // workflows.discard_draft: ordin-host `workflows_discard_draft`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let out = m.workflows_discard_draft(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_restore) => {{
        // workflows.restore: ordin-host `workflows_restore`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let p1 = $api.restate(input.version)?;
        let out = m.workflows_restore(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("version".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_rename) => {{
        // workflows.rename: ordin-host `workflows_rename`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let p1 = $api.restate(input.name)?;
        let out = m.workflows_rename(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_duplicate) => {{
        // workflows.duplicate: ordin-host `workflows_duplicate`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let p1 = $api.restate(input.version)?;
        let p2 = $api.restate(input.name)?;
        let p3 = $api.restate(input.to_workspace_id)?;
        let out = m.workflows_duplicate(p0, p1, p2, p3).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("workflow_id".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_remove) => {{
        // workflows.remove: ordin-host `workflows_remove`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let p1 = $api.restate(input.confirmed)?;
        let out = m.workflows_remove(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_export) => {{
        // workflows.export: ordin-host `workflows_export`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let p1 = $api.restate(input.version)?;
        let out = m.workflows_export(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let (o0, o1) = out;
        let mut obj = ::serde_json::Map::new();
        obj.insert("file_name".to_owned(), $api.value(o0)?);
        obj.insert("yaml".to_owned(), $api.value(o1)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_import) => {{
        // workflows.import: ordin-host `workflows_import`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.file_name)?;
        let p2 = $api.restate(input.yaml)?;
        let out = m.workflows_import(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        let (o0, o1) = out;
        let mut obj = ::serde_json::Map::new();
        obj.insert("workflow_id".to_owned(), $api.value(o0)?);
        obj.insert("check".to_owned(), $api.value(o1)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_list_in_repositories) => {{
        // workflows.list_in_repositories: ordin-host `workflows_list_in_repositories`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.workflows_list_in_repositories(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_add_from_repository) => {{
        // workflows.add_from_repository: ordin-host `workflows_add_from_repository`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.host)?;
        let p2 = $api.restate(input.repository)?;
        let p3 = $api.restate(input.folder)?;
        let p4 = $api.restate(input.file_name)?;
        let out = m.workflows_add_from_repository(p0, p1, p2, p3, p4).await.map_err(|e| $api.module_error(e))?;
        let (o0, o1) = out;
        let mut obj = ::serde_json::Map::new();
        obj.insert("workflow_id".to_owned(), $api.value(o0)?);
        obj.insert("check".to_owned(), $api.value(o1)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_share) => {{
        // workflows.share: ordin-host `workflows_share`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let p1 = $api.restate(input.version)?;
        let p2 = $api.restate(input.folder)?;
        let out = m.workflows_share(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("submission_id".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_send_back) => {{
        // workflows.send_back: ordin-host `workflows_send_back`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let out = m.workflows_send_back(p0).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("submission_id".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_use_newer) => {{
        // workflows.use_newer: ordin-host `workflows_use_newer`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let out = m.workflows_use_newer(p0).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("version".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_keep_as_own) => {{
        // workflows.keep_as_own: ordin-host `workflows_keep_as_own`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let out = m.workflows_keep_as_own(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, workflows_run) => {{
        // workflows.run: ordin-host `workflows_run`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let p0 = $api.restate(input.workflow_id)?;
        let p1 = $api.restate(input.version)?;
        let p2 = $api.restate(input.request)?;
        let out = m.workflows_run(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("crew_charter_id".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, journal_list_entries) => {{
        // journal.list_entries: ordin-journal `list_entries`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input.filter)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.list_entries(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, journal_list_external_actions) => {{
        // journal.list_external_actions: ordin-journal `list_external_actions`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input.state)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.list_external_actions(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, artefacts_get) => {{
        // artefacts.get: ordin-journal `artefact` (SVC-SCR-3, DSK-REC-40.3); `None` answers `{}`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let out = m.artefact(input.artefact_id).await.map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::commands::artefacts::ArtefactsGetResult { artefact: out })
    }};
    ($api:expr, $ctx:expr, $input:expr, artefacts_list) => {{
        // artefacts.list: ordin-journal `list_artefacts`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input.filter)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.list_artefacts(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, artefacts_versions) => {{
        // artefacts.versions: ordin-journal `artefact_versions`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input.location)?;
        let p1: String = $api.restate(input.path)?;
        let out = m.artefact_versions(p0, &p1).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("items".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, reports_get) => {{
        // reports.get: ordin-journal `report`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input.report_id)?;
        let out = m.report(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, reports_list) => {{
        // reports.list: ordin-journal `list_reports`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.list_reports(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, dashboards_list) => {{
        // dashboards.list: ordin-journal `list_dashboards`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.list_dashboards(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, dashboards_keep) => {{
        // dashboards.keep: ordin-journal `keep_dashboard`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.keep_dashboard(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, dashboards_remove) => {{
        // dashboards.remove: ordin-journal `remove_dashboard`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input.dashboard_id)?;
        let out = m.remove_dashboard(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, history_get) => {{
        // history.get: ordin-journal `retention`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let _ = $input;
        let out = m.retention().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, history_set_days) => {{
        // history.set_days: ordin-journal `set_retention_days`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let out = m.set_retention_days(input.days).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, diagnostics_lines) => {{
        // diagnostics.lines: ordin-journal `diagnostic_lines`, each line from a `DiagnosticEntry`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let out = m.diagnostic_lines(input.cursor).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, craft_conversions) => {{
        // craft.conversions: ordin-craft-toolkit `conversion_available` for each copy format
        let _ = ($ctx, $input);
        let m = $api.handle(&$api.modules.craft)?;
        let available = |format| ::serde_json::Value::Bool(m.conversion_available(format));
        let mut obj = ::serde_json::Map::new();
        obj.insert("word".to_owned(), available(::ordin_types::domain::places::CopyFormat::Word));
        obj.insert("pdf".to_owned(), available(::ordin_types::domain::places::CopyFormat::Pdf));
        obj.insert("slides".to_owned(), available(::ordin_types::domain::places::CopyFormat::Slides));
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, files_published_copies) => {{
        // files.published_copies: ordin-files `published_copies_of`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let out = m.published_copies_of(&input.original).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, journal_mark_arrived) => {{
        // journal.mark_arrived: ordin-journal `mark_external_action_arrived`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let out = m
            .mark_external_action_arrived(input.action_key)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, plans_get) => {{
        // plans.get: ordin-journal `current_plan`, as `{ items }`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let out = m.current_plan(input.run_id).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("items".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, journal_run_external_actions) => {{
        // journal.run_external_actions: ordin-journal `external_actions_for_run`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.journal)?;
        let input = $input;
        let p0 = $api.restate(input.run_id)?;
        let out = m.external_actions_for_run(p0).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("items".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, profile_status) => {{
        // profile.status: ordin-organisation-profile `status`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.profile)?;
        let _ = $input;
        let out = m.status();
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, profile_refresh) => {{
        // profile.refresh: ordin-organisation-profile `refresh`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.profile)?;
        let _ = $input;
        let out = m.refresh().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, remote_agents_list) => {{
        // remote_agents.list: ordin-organisation-profile `remote_agents`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.profile)?;
        let _ = $input;
        let out = m.remote_agents().await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("items".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_list) => {{
        // knowledge.list: ordin-knowledge `list_pages`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.filter)?;
        let out = m.list_pages(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_get) => {{
        // knowledge.get: ordin-knowledge `get_page`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let out = m.get_page(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_read_agent_page) => {{
        // knowledge.read_agent_page: ordin-knowledge `read_agent_page`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.agent_instance_id)?;
        let p2: String = $api.restate(input.relative_path)?;
        let out = m.read_agent_page(p0, p1, &p2).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("markdown".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_save_edit) => {{
        // knowledge.save_edit: ordin-knowledge `save_edit`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let p1 = $api.restate(input.title)?;
        let p2 = $api.restate(input.body_markdown)?;
        let p3 = $api.restate(input.tags)?;
        let p4 = $api.restate(input.aliases)?;
        let p5 = $api.restate(input.page_type)?;
        let p6 = $api.restate(input.description)?;
        let out = m.save_edit(p0, p1, p2, p3, p4, p5, p6).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_use_update) => {{
        // knowledge.use_update: ordin-knowledge `use_update`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let out = m.use_update(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_keep_page) => {{
        // knowledge.keep_page: ordin-knowledge `keep_page`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let out = m.keep_page(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_leave_it) => {{
        // knowledge.leave_it: ordin-knowledge `leave_it`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let out = m.leave_it(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_remove) => {{
        // knowledge.remove: ordin-knowledge `remove_page`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let out = m.remove_page(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_undo_remove) => {{
        // knowledge.undo_remove: ordin-knowledge `undo_remove`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let out = m.undo_remove(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_copy_to_workspace) => {{
        // knowledge.copy_to_workspace: ordin-knowledge `copy_to_workspace`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let p1 = $api.restate(input.to_workspace_id)?;
        let out = m.copy_to_workspace(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("page_id".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_publish_to_confluence) => {{
        // knowledge.publish_to_confluence: ordin-knowledge `publish_to_confluence`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let p1 = $api.restate(input.destination)?;
        let p2 = $api.restate(input.keep_up_to_date)?;
        let out = m.publish_to_confluence(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("published_copy_id".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_accept_suggestion) => {{
        // knowledge.accept_suggestion: ordin-knowledge `accept_suggestion`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let p1 = $api.restate(input.suggestion_id)?;
        let out = m.accept_suggestion(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_dismiss_suggestion) => {{
        // knowledge.dismiss_suggestion: ordin-knowledge `dismiss_suggestion`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.page_id)?;
        let p1 = $api.restate(input.suggestion_id)?;
        let out = m.dismiss_suggestion(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_list) => {{
        // memory.list: ordin-memory-service `list_facts`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.cursor)?;
        let out = m.list_facts(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_get) => {{
        // memory.get: ordin-memory-service `get_facts`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.ids)?;
        let out = m.get_facts(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("facts".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_correct) => {{
        // memory.correct: ordin-memory-service `correct_fact`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let p1 = $api.restate(input.object)?;
        let out = m.correct_fact(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_forget) => {{
        // memory.forget: ordin-memory-service `forget_fact`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.forget_fact(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_remember_everywhere) => {{
        // memory.remember_everywhere: ordin-memory-service `remember_everywhere`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.remember_everywhere(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    // The held writes and sources to confirm of one workspace (SVC-SCR-2, DSK-REC-40.2).
    ($api:expr, $ctx:expr, $input:expr, memory_held_writes) => {{
        // memory.held_writes: ordin-memory-service `held_writes`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = ::ordin_memory_service::MemoryPlace::Workspace(input.workspace_id);
        let out = m.held_writes(place).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_keep_held) => {{
        // memory.keep_held: ordin-memory-service `keep_held`; the receipt is dropped
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = ::ordin_memory_service::MemoryPlace::Workspace(input.workspace_id);
        let out = m
            .keep_held(place, &input.id, &input.content_hash)
            .await
            .map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_discard_held) => {{
        // memory.discard_held: ordin-memory-service `discard_held`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = ::ordin_memory_service::MemoryPlace::Workspace(input.workspace_id);
        m.discard_held(place, &input.id).await.map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_citations_to_confirm) => {{
        // memory.citations_to_confirm: ordin-memory-service `citations_to_confirm`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let out = m
            .citations_to_confirm(input.workspace_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_confirm_citation) => {{
        // memory.confirm_citation: ordin-memory-service `confirm_citation`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p1 = $api.restate(input.id)?;
        m.confirm_citation(input.workspace_id, p1)
            .await
            .map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_keep_checked) => {{
        // memory.keep_checked: ordin-memory-service `keep_checked`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.keep_checked(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_list_repositories) => {{
        // catalogue.list_repositories: ordin-catalogue `list_repositories`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let _ = $input;
        let out = m.list_repositories().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_probe_repository) => {{
        // catalogue.probe_repository: ordin-catalogue `probe_repository`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.probe_repository(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_add_repository) => {{
        // catalogue.add_repository: ordin-catalogue `add_repository`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.add_repository(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_rename_repository) => {{
        // catalogue.rename_repository: ordin-catalogue `rename_repository`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.rename_repository(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_remove_repository) => {{
        // catalogue.remove_repository: ordin-catalogue `remove_repository`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.remove_repository(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_fetch_now) => {{
        // catalogue.fetch_now: ordin-catalogue `fetch_now`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.fetch_now(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_search) => {{
        // catalogue.search: ordin-catalogue `search`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.search(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_facets) => {{
        // catalogue.facets: ordin-catalogue `facets`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.facets(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_part) => {{
        // catalogue.part: ordin-catalogue `part`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.part(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_read_part) => {{
        // catalogue.read_part: ordin-catalogue `read_part`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.read_part(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_used_by) => {{
        // catalogue.used_by: ordin-catalogue `used_by`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.used_by(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_made_of) => {{
        // catalogue.made_of: ordin-catalogue `made_of`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.agent)?;
        let out = m.made_of(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_part_history) => {{
        // catalogue.part_history: ordin-catalogue `part_history`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.part_history(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_health) => {{
        // catalogue.health: ordin-catalogue `health`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.health(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_withdrawn_list) => {{
        // catalogue.withdrawn_list: ordin-catalogue `withdrawn_list`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.withdrawn_list(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_vocabulary) => {{
        // catalogue.vocabulary: ordin-catalogue `vocabulary`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.vocabulary(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_layout) => {{
        // catalogue.layout: ordin-catalogue `layout`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.layout(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_lifecycles) => {{
        // catalogue.lifecycles: ordin-catalogue `lifecycles`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.lifecycles(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, catalogue_lifecycle) => {{
        // catalogue.lifecycle: ordin-catalogue `lifecycle`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.catalogue)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.lifecycle(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, checker_report) => {{
        // checker.report: ordin-checker `report`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.checker)?;
        let input = $input;
        let p0 = $api.restate(input.check_run_id)?;
        let out = m.report(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_preview_add) => {{
        // agents.preview_add: ordin-package-catalogue `preview_add`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.packages)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.preview_add(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_add) => {{
        // agents.add: ordin-package-catalogue `add`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.packages)?;
        let input = $input;
        let mut p0: ::ordin_package_catalogue::AddAgentInput = $api.restate(input)?;
        // PackageCatalogue.md (*Service API commands*): the Service API fills `workspace_languages` from the
        // Project Service. A build without the Project Service leaves them as the caller sent them.
        if let Ok(projects) = $api.handle(&$api.modules.projects) {
            p0.workspace_languages = projects
                .workspace_languages(p0.workspace_id)
                .await
                .map_err(|e| $api.module_error(e))?;
        }
        let out = m.add(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_list) => {{
        // agents.list: ordin-package-catalogue `list`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.packages)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let out = m.list(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_get) => {{
        // agents.get: ordin-package-catalogue `get`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.packages)?;
        let input = $input;
        let p0 = $api.restate(input.agent_instance_id)?;
        let out = m.get(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_change_choice) => {{
        // agents.change_choice: ordin-package-catalogue `change_choice`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.packages)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.change_choice(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_accept_update) => {{
        // agents.accept_update: ordin-package-catalogue `accept_update`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.packages)?;
        let input = $input;
        let p0 = $api.restate(input.agent_instance_id)?;
        let out = m.accept_update(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_decline_update) => {{
        // agents.decline_update: ordin-package-catalogue `decline_update`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.packages)?;
        let input = $input;
        let p0 = $api.restate(input.agent_instance_id)?;
        let out = m.decline_update(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_remove) => {{
        // agents.remove: ordin-package-catalogue `remove`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.packages)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.remove(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_made_of) => {{
        // agents.made_of: ordin-package-inspector `made_of`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.inspector)?;
        let input = $input;
        let p0 = $api.restate(input.agent_instance_id)?;
        let p1 = $api.restate(input.detail)?;
        let out = m.made_of(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_open) => {{
        // projects.open: ordin-projects `open_project`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.place_id)?;
        let p1 = $api.restate(input.workspace_id)?;
        let out = m.open_project(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_get) => {{
        // projects.get: ordin-projects `project` (SVC-SCR-4, DSK-REC-40.4)
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let out = m.project(input.project_id).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_close) => {{
        // projects.close: ordin-projects `close_project`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let out = m.close_project(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_status) => {{
        // projects.status: ordin-projects `status`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let out = m.status(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_diff) => {{
        // projects.diff: ordin-projects `diff`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1: Option<String> = $api.restate(input.path)?;
        let out = m.diff(p0, p1.as_deref()).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_history) => {{
        // projects.history: ordin-projects `history`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1: Option<String> = $api.restate(input.path)?;
        let p2 = $api.restate(input.limit)?;
        let out = m.history(p0, p1.as_deref(), p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_branches) => {{
        // projects.branches: ordin-projects `branches`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let out = m.branches(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_create_branch) => {{
        // projects.create_branch: ordin-projects `create_branch`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1: String = $api.restate(input.name)?;
        let out = m.create_branch(p0, &p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_switch_branch) => {{
        // projects.switch_branch: ordin-projects `switch_branch`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1: String = $api.restate(input.name)?;
        let out = m.switch_branch(p0, &p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_commit) => {{
        // projects.commit: ordin-projects `commit`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1: String = $api.restate(input.message)?;
        let p2 = $api.restate(input.terminal)?;
        let out = m.commit(p0, &p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_push) => {{
        // projects.push: ordin-projects `push`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1 = $api.restate(input.terminal)?;
        let out = m.push(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_save_file) => {{
        // projects.save_file: ordin-projects `save_file`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1: String = $api.restate(input.path)?;
        let p2 = $api.restate(input.text)?;
        let p3 = $api.restate(input.expected_hash)?;
        let out = m.save_file(p0, &p1, p2, p3).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_review) => {{
        // projects.review: ordin-projects `review`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let out = m.review(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_decide_entry) => {{
        // projects.decide_entry: ordin-projects `decide_entry`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.entry)?;
        let p1 = $api.restate(input.decision)?;
        let out = m.decide_entry(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_keep_attempt) => {{
        // projects.keep_attempt: ordin-projects `keep_attempt`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.task)?;
        let out = m.keep_attempt(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_run_check) => {{
        // projects.run_check: ordin-projects `run_check`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1 = $api.restate(input.task)?;
        let p2: String = $api.restate(input.command)?;
        let p3 = $api.restate(input.origin)?;
        let out = m.run_check(p0, p1, &p2, p3).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_git_host_settings) => {{
        // projects.git_host_settings: ordin-projects `git_host_settings`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let out = m.git_host_settings(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_set_git_host_approval) => {{
        // projects.set_git_host_approval: ordin-projects `set_git_host_approval`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1: String = $api.restate(input.kind)?;
        let p2 = $api.restate(input.approval_required)?;
        let out = m.set_git_host_approval(p0, &p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_language_plugins) => {{
        // projects.language_plugins: ordin-projects `language_plugins`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let _ = $input;
        let out = m.language_plugins().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_configure_language) => {{
        // projects.configure_language: ordin-projects `configure_language`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0: String = $api.restate(input.language)?;
        let p1 = $api.restate(input.chosen_server)?;
        let p2 = $api.restate(input.enabled)?;
        let p3 = $api.restate(input.server_settings)?;
        let out = m.configure_language(&p0, p1, p2, p3).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_restart_language_server) => {{
        // projects.restart_language_server: ordin-projects `restart_language_server`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0: String = $api.restate(input.language)?;
        let out = m.restart_language_server(&p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_language_servers_answer) => {{
        // projects.language_servers_answer: ordin-projects `language_servers_answer`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let out = m.language_servers_answer(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_answer_language_servers) => {{
        // projects.answer_language_servers: ordin-projects `answer_language_servers`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1 = $api.restate(input.allowed)?;
        let out = m.answer_language_servers(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_editor_open) => {{
        // projects.editor_open: ordin-projects `editor_open`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1: String = $api.restate(input.path)?;
        let out = m.editor_open(p0, &p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_lsp_send) => {{
        // projects.lsp_send: ordin-projects `lsp_send`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.session)?;
        let p1 = $api.restate(input.message)?;
        let out = m.lsp_send(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_editor_close) => {{
        // projects.editor_close: ordin-projects `editor_close`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.session)?;
        let out = m.editor_close(p0).await;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_ides) => {{
        // projects.ides: ordin-projects `ides`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let _ = $input;
        let out = m.ides().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_set_ide_enabled) => {{
        // projects.set_ide_enabled: ordin-projects `set_ide_enabled`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0: String = $api.restate(input.ide_id)?;
        let p1 = $api.restate(input.enabled)?;
        let out = m.set_ide_enabled(&p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_open_in_ide) => {{
        // projects.open_in_ide: ordin-projects `open_in_ide`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1: String = $api.restate(input.ide_id)?;
        let p2: Option<String> = $api.restate(input.path)?;
        let out = m.open_in_ide(p0, &p1, p2.as_deref()).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_terminal_open) => {{
        // projects.terminal_open: ordin-projects `terminal_open`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1 = $api.restate(input.cols)?;
        let p2 = $api.restate(input.rows)?;
        let out = m.terminal_open(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_terminal_input) => {{
        // projects.terminal_input: ordin-projects `terminal_input`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.terminal)?;
        let p1 = $api.restate(input.bytes)?;
        let out = m.terminal_input(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_terminal_resize) => {{
        // projects.terminal_resize: ordin-projects `terminal_resize`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.terminal)?;
        let p1 = $api.restate(input.cols)?;
        let p2 = $api.restate(input.rows)?;
        let out = m.terminal_resize(p0, p1, p2).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_terminal_close) => {{
        // projects.terminal_close: ordin-projects `terminal_close`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.terminal)?;
        let out = m.terminal_close(p0).await;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_check_now) => {{
        // projects.check_now: ordin-projects `fetch_followed`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.repo_place)?;
        let p1: String = $api.restate(input.branch)?;
        let out = m.fetch_followed(p0, &p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, projects_set_check_commands) => {{
        // projects.set_check_commands: ordin-projects `set_check_commands`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.projects)?;
        let input = $input;
        let p0 = $api.restate(input.project_id)?;
        let p1 = $api.restate(input.commands)?;
        let out = m.set_check_commands(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, usage_update_settings) => {{
        // usage.update_settings: ordin-usage-planner `update_settings`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.usage)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.update_settings(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, usage_get_settings) => {{
        // usage.get_settings: ordin-usage-planner `settings`
        let _ = ($ctx, $input);
        let m = $api.handle(&$api.modules.usage)?;
        $api.restate(m.settings())
    }};
    ($api:expr, $ctx:expr, $input:expr, usage_today) => {{
        // usage.today: ordin-usage-planner `today`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.usage)?;
        let _ = $input;
        let out = m.today().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, identity_list) => {{
        // identity.list: ordin-identity `list`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.identity)?;
        let _ = $input;
        let out = m.list().await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("items".to_owned(), $api.value(out)?);
        // SM-NS-2.7: GitHub's one-click sign-in is drawn only when this build carries a GitHub App.
        obj.insert(
            "github_one_click".to_owned(),
            ::serde_json::Value::Bool(m.github_app_offered()),
        );
        // ID-MS-BUILT-3: Microsoft 365 can be signed in to without a profile when the build carries its app.
        obj.insert(
            "microsoft_built_in".to_owned(),
            ::serde_json::Value::Bool(m.microsoft_built_in().is_some()),
        );
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, identity_sign_in_cancel) => {{
        // identity.sign_in_cancel: ordin-identity `cancel_sign_in`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.identity)?;
        let input = $input;
        let p0 = $api.restate(input.challenge_id)?;
        let out = m.cancel_sign_in(p0).map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, identity_sign_out) => {{
        // identity.sign_out: ordin-identity `sign_out`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.identity)?;
        let input = $input;
        let p0 = $api.restate(input.credential_id)?;
        let out = m.sign_out(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_run_guided) => {{
        // setup.run_guided: ordin-setup `run_guided`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.setup)?;
        let _ = $input;
        let out = m.run_guided().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_sign_in_harness) => {{
        // setup.sign_in_harness: ordin-setup `sign_in_harness`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.setup)?;
        let input = $input;
        let p0 = $api.restate(input.harness)?;
        let out = m.sign_in_harness(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_update_offer) => {{
        // setup.update_offer: ordin-setup `update_offer`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.setup)?;
        let _ = $input;
        let out = m.update_offer();
        let mut obj = ::serde_json::Map::new();
        obj.insert("offer".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_install_update) => {{
        // setup.install_update: ordin-setup `install_update`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.setup)?;
        let _ = $input;
        let out = m.install_update().await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_set_locale) => {{
        // localisation.set_locale: ordin-localisation `set_locale`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: Option<String> = $api.restate(input.locale)?;
        let out = m.set_locale(p0.as_deref()).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_get_catalogue) => {{
        // localisation.get_catalogue: ordin-localisation `catalogue`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: String = $api.restate(input.locale)?;
        let out = m.catalogue(&p0).map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_edit_message) => {{
        // localisation.edit_message: ordin-localisation `edit_message`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: String = $api.restate(input.locale)?;
        let p1: String = $api.restate(input.message_id)?;
        let p2: String = $api.restate(input.ftl)?;
        let out = m.edit_message(&p0, &p1, &p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_restore_message) => {{
        // localisation.restore_message: ordin-localisation `restore_message`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: String = $api.restate(input.locale)?;
        let p1: String = $api.restate(input.message_id)?;
        let out = m.restore_message(&p0, &p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_restore_language) => {{
        // localisation.restore_language: ordin-localisation `restore_language`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: String = $api.restate(input.locale)?;
        let out = m.restore_language(&p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_mark_reviewed) => {{
        // localisation.mark_reviewed: ordin-localisation `mark_reviewed`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: String = $api.restate(input.locale)?;
        let p1: Vec<_> = $api.restate(input.message_ids)?;
        let out = m.mark_reviewed(&p0, p1.as_slice()).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_confirm_language) => {{
        // localisation.confirm_language: ordin-localisation `confirm_language`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: String = $api.restate(input.locale)?;
        let out = m.confirm_language(&p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_remove_language) => {{
        // localisation.remove_language: ordin-localisation `remove_language`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: String = $api.restate(input.locale)?;
        let out = m.remove_language(&p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_export_language) => {{
        // localisation.export_language: ordin-localisation `export_language`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: String = $api.restate(input.locale)?;
        let out = m.export_language(&p0).map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_import_language) => {{
        // localisation.import_language: ordin-localisation `import_language`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let p0: String = $api.restate(input.file_name)?;
        let p1: String = $api.restate(input.ftl)?;
        let p2 = $api.restate(input.replace)?;
        let out = m.import_language(&p0, &p1, p2).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, servers_list) => {{
        // servers.list: ordin-server-connector `list`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.connector)?;
        let _ = $input;
        let out = m.list().await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, servers_connect) => {{
        // servers.connect: ordin-server-connector `connect`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.connector)?;
        let input = $input;
        let p0 = $api.restate(input)?;
        let out = m.connect(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, servers_disconnect) => {{
        // servers.disconnect: ordin-server-connector `disconnect`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.connector)?;
        let input = $input;
        let p0 = $api.restate(input.server_connection_id)?;
        let out = m.disconnect(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, servers_remove) => {{
        // servers.remove: ordin-server-connector `remove`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.connector)?;
        let input = $input;
        let p0 = $api.restate(input.server_connection_id)?;
        let out = m.remove(p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_list_read_pages) => {{
        // knowledge.list_read_pages: ordin-knowledge `list_read_pages`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.folder_id)?;
        let out = m.list_read_pages(p0, p1).await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("items".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_fact_history) => {{
        // memory.fact_history: ordin-memory-service `fact_history`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.fact_history(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_fact_origins) => {{
        // memory.fact_origins: ordin-memory-service `fact_origins`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.fact_origins(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_keep) => {{
        // memory.keep: ordin-memory-service `keep_fact`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.keep_fact(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_remembered_in_run) => {{
        // memory.remembered_in_run: ordin-memory-service `remembered_in_run`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.run_id)?;
        let out = m.remembered_in_run(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_correct_link) => {{
        // memory.correct_link: ordin-memory-service `correct_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let p1 = $api.restate(input.kind)?;
        let p2 = $api.restate(input.from)?;
        let p3 = $api.restate(input.to)?;
        let out = m.correct_link(p0, p1, p2, p3).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_forget_link) => {{
        // memory.forget_link: ordin-memory-service `forget_link`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.forget_link(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_forget_count) => {{
        // memory.forget_count: ordin-memory-service `forget_count`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.id)?;
        let out = m.forget_count(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_more_found) => {{
        // memory.more_found: ordin-memory-service `more_found`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let p0 = $api.restate(input.workspace_id)?;
        let p1 = $api.restate(input.recall_id)?;
        let out = m.more_found(p0, p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_check_update) => {{
        // setup.check_update: ordin-setup `check_update`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.setup)?;
        let _ = $input;
        let out = m.check_update().await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("offer".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_whats_new) => {{
        // setup.whats_new: ordin-setup `whats_new`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.setup)?;
        let _ = $input;
        let out = m.whats_new().await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("whats_new".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_whats_new_seen) => {{
        // setup.whats_new_seen: ordin-setup `whats_new_seen`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.setup)?;
        let input = $input;
        let p0: String = $api.restate(input.version)?;
        let out = m.whats_new_seen(&p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_server_update) => {{
        // setup.server_update: ordin-setup `server_update`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.setup)?;
        let _ = $input;
        let out = m.server_update().await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("update".to_owned(), $api.value(out)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_update_server) => {{
        // setup.update_server: ordin-setup `update_server`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.setup)?;
        let input = $input;
        let p0: String = $api.restate(input.version)?;
        let out = m.update_server(&p0).await.map_err(|e| $api.module_error(e))?;
        let _ = out;
        let mut obj = ::serde_json::Map::new();
        obj.insert("accepted".to_owned(), ::serde_json::Value::Bool(true));
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_status) => {{
        // setup.status: ordin-setup `needs_guided_setup`, `progress` and `harness_attachments`
        let _ = ($ctx, $input);
        let m = $api.handle(&$api.modules.setup)?;
        let needs_guided = m.needs_guided_setup().await.map_err(|e| $api.module_error(e))?;
        let progress = m.progress().borrow().clone();
        let attachments = m.harness_attachments().await.map_err(|e| $api.module_error(e))?;
        let mut obj = ::serde_json::Map::new();
        obj.insert("needs_guided".to_owned(), ::serde_json::Value::Bool(needs_guided));
        obj.insert("progress".to_owned(), $api.value(progress)?);
        obj.insert("attachments".to_owned(), $api.value(attachments)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, setup_check_now) => {{
        // setup.check_now: ordin-setup `run_checks(OnDemand)`
        let _ = ($ctx, $input);
        let m = $api.handle(&$api.modules.setup)?;
        let out = m
            .run_checks(::ordin_setup::CheckTrigger::OnDemand)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, localisation_list_messages) => {{
        // localisation.list_messages: ordin-localisation `messages`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.localisation)?;
        let input = $input;
        let filter: ::ordin_localisation::MessageFilter = $api.restate(::serde_json::json!({
            "text": input.text,
            "area": input.area,
            "only": input.only,
        }))?;
        let cursor = $api.restate(input.cursor)?;
        let out = m
            .messages(&input.locale, &filter, cursor)
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    // The rows of FU-149 (Phase 13A) whose owner method is the one the row's *Note* names; the
    // spec for each is ServiceApi.md SVC-R1-1.
    // spec: ordin-desktop/components/ServiceApi#SVC-R1-1
    ($api:expr, $ctx:expr, $input:expr, brand_get_asset) => {{
        // brand.get_asset: ordin-brand `asset`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.brand)?;
        let input = $input;
        let p0 = $api.restate(input.asset)?;
        let out = m.asset(p0).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, ordin_draft_language) => {{
        // ordin.draft_language: ordin-host `draft_language`; the reply is `{ accepted: true, messages }`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.ordin_host)?;
        let input = $input;
        let messages = m
            .draft_language(input.locale, input.name)
            .await
            .map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::commands::ordin::OrdinDraftLanguageResult {
            accepted: true,
            messages,
        })
    }};
    ($api:expr, $ctx:expr, $input:expr, craft_open) => {{
        // craft.open: ordin-craft-toolkit `open_in_app`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.craft)?;
        let input = $input;
        let p0 = $api.restate(input.place_id)?;
        let p1: String = $api.restate(input.path)?;
        let out = m.open_in_app(p0, &p1).await.map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, confluence_find) => {{
        // confluence.find: ordin-confluence `find_locations`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.confluence)?;
        let input = $input;
        let out = m
            .find_locations(&input.query, input.limit)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, confluence_list) => {{
        // confluence.list: ordin-confluence `list_children`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.confluence)?;
        let input = $input;
        let location = $api.restate(input.location)?;
        let cursor = $api.restate(input.cursor)?;
        let out = m
            .list_children(&location, cursor)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, sharepoint_find) => {{
        // sharepoint.find: ordin-sharepoint `find_locations`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.sharepoint)?;
        let input = $input;
        let out = m
            .find_locations(&input.query, input.limit)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, sharepoint_list) => {{
        // sharepoint.list: ordin-sharepoint `list_children`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.sharepoint)?;
        let input = $input;
        let location = $api.restate(input.location)?;
        let cursor = $api.restate(input.cursor)?;
        let out = m
            .list_children(&location, cursor)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, knowledge_write) => {{
        // knowledge.write: ordin-knowledge `write_page` by the person, a new page with no sources
        let _ = $ctx;
        let m = $api.handle(&$api.modules.knowledge)?;
        let input = $input;
        let page_id = m
            .write_page(
                ::ordin_types::domain::KnowledgeAuthor::Person,
                ::ordin_knowledge::WritePage {
                    workspace_id: input.workspace_id,
                    page_id: None,
                    title: input.title,
                    body_markdown: input.body_markdown,
                    sources: Vec::new(),
                    run_id: None,
                    tags: None,
                    aliases: None,
                    page_type: input.page_type,
                    description: input.description,
                },
            )
            .await
            .map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::commands::knowledge::KnowledgeWriteResult { page_id })
    }};
    ($api:expr, $ctx:expr, $input:expr, files_add_place) => {{
        // files.add_place: ordin-files `add_place`, a folder the person picked
        let _ = $ctx;
        let m = $api.handle(&$api.modules.files)?;
        let input = $input;
        let out = m
            .add_place(::ordin_files::AddPlace {
                path: ::std::path::PathBuf::from(input.path),
                friendly_name: input.friendly_name,
                kind: ::ordin_types::domain::PlaceKind::PersonsFolder,
            })
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, jira_connect_token) => {{
        // jira.connect_token: ordin-jira `connect_with_token`; the token goes in as a secret and is
        // never logged or echoed (SA-JIRA-04)
        let _ = $ctx;
        let m = $api.handle(&$api.modules.jira)?;
        let input = $input;
        let out = m
            .connect_with_token(
                &input.site,
                &input.email,
                ::ordin_identity::SecretString::new(input.token),
            )
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, confluence_connect_token) => {{
        // confluence.connect_token: ordin-confluence `connect_with_token`; the token goes in as a secret
        // and is never logged or echoed (SVC-CONF-TOKEN-1)
        let _ = $ctx;
        let m = $api.handle(&$api.modules.confluence)?;
        let input = $input;
        let out = m
            .connect_with_token(
                &input.site,
                &input.email,
                ::ordin_identity::SecretString::new(input.token),
            )
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, confluence_connect_with_jira) => {{
        // confluence.connect_with_jira: the Jira API token's site, email and token, read through
        // Identity, checked by ordin-confluence `connect_with_token` as if pasted; with no Jira API token
        // it is `JIRA_NOT_CONNECTED` (SVC-CONF-TOKEN-1)
        let _ = ($ctx, $input);
        let m = $api.handle(&$api.modules.confluence)?;
        let identity = $api.handle(&$api.modules.identity)?;
        let access = identity
            .jira_access()
            .await
            .map_err(|e| $api.module_error(e))?
            .ok_or_else(|| $api.module_error(::ordin_jira::JiraError::NotConnected))?;
        let ::ordin_identity::JiraCredentialMethod::ApiToken { email } = access.method else {
            return Err($api.module_error(::ordin_jira::JiraError::NotConnected));
        };
        let out = m
            .connect_with_token(&access.site, &email, access.token.token)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, jira_find_issues) => {{
        // jira.find_issues: ordin-jira `search` with `JiraQuery::Text` on the connected site
        let _ = $ctx;
        let m = $api.handle(&$api.modules.jira)?;
        let identity = $api.handle(&$api.modules.identity)?;
        let input = $input;
        let access = identity
            .jira_access()
            .await
            .map_err(|e| $api.module_error(e))?
            .ok_or_else(|| $api.module_error(::ordin_jira::JiraError::NotConnected))?;
        let out = m
            .search(
                &access.site,
                &::ordin_jira::JiraQuery::Text(input.text),
                input.limit,
            )
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, confluence_keep_mine) => {{
        // confluence.keep_mine: ordin-confluence `keep_local`
        let _ = $ctx;
        let m = $api.handle(&$api.modules.confluence)?;
        let input = $input;
        let p0 = $api.restate(input.published_copy_id)?;
        m.keep_local(p0).await.map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::Done {})
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_add_email_link) => {{
        // workspaces.add_email_link: ordin-workspaces `propose_email_link`, then `confirm_email_link`
        // (the person pressed the button, so the link is proposed and confirmed in one call)
        let _ = $ctx;
        let m = $api.handle(&$api.modules.workspaces)?;
        let input = $input;
        let link = ::ordin_workspaces::NewEmailLink {
            kind: $api.restate(input.kind)?,
            folder_or_label: input.folder_or_label,
            addresses: input.addresses,
            domains: input.domains,
            display_name: input.display_name,
        };
        let proposed = m
            .propose_email_link(input.workspace_id, link)
            .await
            .map_err(|e| $api.module_error(e))?;
        let out = m
            .confirm_email_link(proposed.id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, $other:ident) => {{
        let _ = ($ctx, $input);
        Err($api.not_answered())
    }};
}
