//! Plugin actions offered in the workspace right-click menu.
//!
//! Plugin v1 lets an action declare `contexts = ["workspace"]`, but nothing
//! rendered them: the context menus were `&'static str` literals. The menu now
//! appends the actions resolved here after its own items, and dispatches a pick
//! back through the same command path a keybinding uses.

use crate::api::schema::PluginActionContext;
use crate::app::state::{AppState, ContextMenuKind, ContextMenuPluginItem};
use crate::app::App;

use super::manifest::{effective_platforms, ensure_platform_supported};
use super::{manifest_action_info, plugin_manifest_available};

impl AppState {
    /// The plugin actions to append to a workspace context menu, in the order
    /// they should appear.
    ///
    /// Read from the cached registry — loaded at startup and refreshed by every
    /// plugin API call, including `plugin link` — so opening a menu never waits
    /// on disk. Actions are filtered to the ones that could actually run: an
    /// enabled plugin whose manifest is readable, declaring the workspace
    /// context, and supported on this platform.
    ///
    /// `plugins.workspace_menu` decides which workspaces get them at all, and
    /// by default that is git ones only: a plugin's workspace actions are
    /// almost always git actions, and a plain directory gives them nothing to
    /// act on.
    pub(crate) fn workspace_menu_plugins(&self, is_git_workspace: bool) -> WorkspaceMenuPlugins {
        use crate::config::WorkspaceMenuConfig;
        match self.plugin_workspace_menu {
            WorkspaceMenuConfig::None => return WorkspaceMenuPlugins::default(),
            WorkspaceMenuConfig::Git if !is_git_workspace => {
                return WorkspaceMenuPlugins::default()
            }
            _ => {}
        }

        let allowlist = &self.plugin_workspace_menu_actions;
        let mut replaced: Vec<(String, &'static str)> = Vec::new();
        let mut items: Vec<ContextMenuPluginItem> = Vec::new();
        for plugin in self.installed_plugins.values() {
            if !plugin.enabled || !plugin_manifest_available(plugin) {
                continue;
            }
            for action in &plugin.actions {
                if !action.contexts.contains(&PluginActionContext::Workspace) {
                    continue;
                }
                let info = manifest_action_info(&plugin.plugin_id, &plugin.platforms, action);
                if ensure_platform_supported(
                    effective_platforms(&info.platforms, &plugin.platforms),
                    &info.qualified_id(),
                )
                .is_err()
                {
                    continue;
                }
                let item = ContextMenuPluginItem {
                    plugin_id: plugin.plugin_id.clone(),
                    action_id: action.id.clone(),
                    title: action.title.clone(),
                };
                // Kept beside the item, not applied yet: a built-in only goes
                // away while the action claiming it survives the allowlist too.
                for id in &action.replaces {
                    if let Some(label) = crate::api::schema::builtin_menu_item_label(id) {
                        replaced.push((item.qualified_id(), label));
                    }
                }
                items.push(item);
            }
        }

        if !allowlist.is_empty() {
            // An allowlist states the order too, and silently drops ids that name
            // a plugin or action this session can't run.
            items = allowlist
                .iter()
                .filter_map(|id| {
                    items
                        .iter()
                        .find(|item| item.qualified_id() == *id)
                        .cloned()
                })
                .collect();
        } else {
            // The registry is a HashMap, so without this the menu would reshuffle
            // itself between opens.
            items.sort_by(|left, right| {
                (&left.plugin_id, &left.action_id).cmp(&(&right.plugin_id, &right.action_id))
            });
        }

        // Drop a built-in only while the action that supersedes it is on offer;
        // an allowlist that leaves that action out gets the built-in back rather
        // than a menu with no way to do the thing at all.
        let hidden_builtins = replaced
            .into_iter()
            .filter(|(qualified_id, _)| {
                items
                    .iter()
                    .any(|item| item.qualified_id() == *qualified_id)
            })
            .map(|(_, label)| label)
            .collect::<Vec<_>>();

        WorkspaceMenuPlugins {
            items,
            hidden_builtins,
        }
    }
}

/// What a plugin contributes to one workspace context menu.
#[derive(Debug, Default)]
pub(crate) struct WorkspaceMenuPlugins {
    /// Actions to list after Herdr's own items.
    pub(crate) items: Vec<ContextMenuPluginItem>,
    /// Built-in item labels to leave out, because a listed action replaces them.
    pub(crate) hidden_builtins: Vec<&'static str>,
}

impl App {
    /// Run the plugin action a context menu item names, against the workspace
    /// the menu was opened on.
    ///
    /// Failures surface as a toast, the way a plugin keybinding's do: the menu
    /// has already closed by the time a command could fail, so there is nowhere
    /// else to report it.
    pub(crate) fn invoke_context_menu_plugin_action(
        &mut self,
        kind: &ContextMenuKind,
        item: &ContextMenuPluginItem,
    ) {
        let ws_idx = match *kind {
            ContextMenuKind::Workspace { ws_idx }
            | ContextMenuKind::GitWorkspace { ws_idx, .. }
            | ContextMenuKind::Tab { ws_idx, .. }
            | ContextMenuKind::Pane { ws_idx, .. } => ws_idx,
        };

        if let Err(message) = self.start_context_menu_plugin_action(ws_idx, item) {
            let previous_toast = self.state.toast.clone();
            self.state.toast = Some(crate::app::state::ToastNotification {
                kind: crate::app::state::ToastKind::NeedsAttention,
                title: format!("{} failed", item.qualified_id()),
                context: message,
                position: None,
                target: None,
            });
            self.sync_toast_deadline(previous_toast);
        }
    }

    /// The context a menu-invoked action runs with: the workspace whose row was
    /// right-clicked, which is not necessarily the focused one.
    fn context_menu_plugin_context(
        &self,
        ws_idx: usize,
    ) -> crate::api::schema::PluginInvocationContext {
        let mut context = self.plugin_context_for_workspace(ws_idx, "context_menu");
        context.invocation_source = Some("context_menu".to_string());
        context
    }

    /// The keybinding path's checks, against the clicked workspace rather than
    /// the focused one.
    fn start_context_menu_plugin_action(
        &mut self,
        ws_idx: usize,
        item: &ContextMenuPluginItem,
    ) -> Result<(), String> {
        self.refresh_installed_plugins()
            .map_err(|err| format!("failed to load plugin registry: {err}"))?;
        let (plugin, action) = self
            .find_plugin_action(Some(&item.plugin_id), &item.action_id)
            .map_err(|(_, message)| message)?;
        if !plugin.enabled {
            return Err(format!("plugin {} is disabled", plugin.plugin_id));
        }
        ensure_platform_supported(
            effective_platforms(&action.platforms, &plugin.platforms),
            &action.qualified_id(),
        )
        .map_err(|(_, message)| message)?;

        let context = self.context_menu_plugin_context(ws_idx);
        self.start_plugin_command(
            &plugin,
            Some(action.action_id),
            None,
            action.command,
            &context,
            None,
        )
        .map(|_| ())
        .map_err(|(_, message)| message)
    }
}

#[cfg(test)]
mod tests {
    use crate::api::schema::{InstalledPluginInfo, PluginActionContext, PluginPlatform};
    use crate::app::state::{AppState, ContextMenuKind, ContextMenuPluginItem};
    use crate::config::WorkspaceMenuConfig;

    use crate::app::state::{test_plugin_action as action, test_plugin_info as plugin};

    fn replacing_action(
        id: &str,
        title: &str,
        replaces: Vec<&str>,
    ) -> crate::api::schema::PluginManifestAction {
        let mut declared = action(id, title, vec![PluginActionContext::Workspace]);
        declared.replaces = replaces.into_iter().map(str::to_string).collect();
        declared
    }

    /// A platform this build is definitely not running on.
    fn foreign_platform() -> PluginPlatform {
        if cfg!(target_os = "windows") {
            PluginPlatform::Linux
        } else {
            PluginPlatform::Windows
        }
    }

    fn state_with(plugins: Vec<InstalledPluginInfo>) -> AppState {
        let mut state = AppState::test_new();
        state.install_test_plugins(plugins);
        state
    }

    fn titles(state: &AppState) -> Vec<String> {
        titles_for(state, true)
    }

    fn titles_for(state: &AppState, is_git_workspace: bool) -> Vec<String> {
        state
            .workspace_menu_plugins(is_git_workspace)
            .items
            .into_iter()
            .map(|item| item.title)
            .collect()
    }

    #[test]
    fn workspace_actions_are_listed_in_a_stable_order() {
        let state = state_with(vec![
            plugin(
                "zebra",
                vec![action("b", "Zebra B", vec![PluginActionContext::Workspace])],
            ),
            plugin(
                "alpha",
                vec![
                    action("b", "Alpha B", vec![PluginActionContext::Workspace]),
                    action("a", "Alpha A", vec![PluginActionContext::Workspace]),
                ],
            ),
        ]);

        assert_eq!(titles(&state), ["Alpha A", "Alpha B", "Zebra B"]);
    }

    #[test]
    fn actions_that_could_not_run_are_left_out() {
        let mut pane_only = plugin(
            "pane-only",
            vec![action("p", "Pane", vec![PluginActionContext::Pane])],
        );
        pane_only.plugin_id = "pane-only".into();

        let mut disabled = plugin(
            "disabled",
            vec![action(
                "d",
                "Disabled",
                vec![PluginActionContext::Workspace],
            )],
        );
        disabled.enabled = false;

        let mut unavailable = plugin(
            "unavailable",
            vec![action(
                "u",
                "Unavailable",
                vec![PluginActionContext::Workspace],
            )],
        );
        unavailable.warnings = vec![format!(
            "{}manifest is gone",
            crate::persist::plugin_registry::MANIFEST_UNAVAILABLE_WARNING_PREFIX
        )];

        let mut foreign = plugin(
            "foreign",
            vec![action("f", "Foreign", vec![PluginActionContext::Workspace])],
        );
        foreign.actions[0].platforms = Some(vec![foreign_platform()]);

        let ok = plugin(
            "ok",
            vec![action("o", "Kept", vec![PluginActionContext::Workspace])],
        );

        let state = state_with(vec![pane_only, disabled, unavailable, foreign, ok]);

        assert_eq!(titles(&state), ["Kept"]);
    }

    #[test]
    fn none_hides_every_plugin_action() {
        let mut state = state_with(vec![plugin(
            "worktrunk",
            vec![action("open", "Open", vec![PluginActionContext::Workspace])],
        )]);
        state.plugin_workspace_menu = WorkspaceMenuConfig::None;

        assert!(state.workspace_menu_plugins(true).items.is_empty());
    }

    #[test]
    fn an_allowlist_picks_the_actions_and_their_order() {
        let mut state = state_with(vec![plugin(
            "worktrunk",
            vec![
                action("open", "Open", vec![PluginActionContext::Workspace]),
                action("issue", "Issue", vec![PluginActionContext::Workspace]),
                action("pr", "PR", vec![PluginActionContext::Workspace]),
            ],
        )]);
        state.plugin_workspace_menu_actions = vec![
            "worktrunk.pr".into(),
            "worktrunk.issue".into(),
            "worktrunk.missing".into(),
        ];

        assert_eq!(titles(&state), ["PR", "Issue"]);
    }

    fn test_app() -> crate::app::App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        crate::app::App::new(
            &crate::config::Config::default(),
            true,
            None,
            api_rx,
            crate::api::EventHub::default(),
        )
    }

    /// A command that exits immediately on either platform: these tests are
    /// about routing, not about what the action does.
    fn noop_command() -> Vec<String> {
        if cfg!(windows) {
            vec!["cmd".into(), "/c".into(), "exit".into()]
        } else {
            vec!["sh".into(), "-c".into(), ":".into()]
        }
    }

    /// A distinct plugin id on purpose: starting a command creates that
    /// plugin's real config and state directories, so these tests must not
    /// borrow the id of an installed plugin or of another test's fixture.
    fn menu_item() -> ContextMenuPluginItem {
        ContextMenuPluginItem {
            plugin_id: "example.menu".into(),
            action_id: "from-issue".into(),
            title: "Worktree: from an issue".into(),
        }
    }

    /// Right-clicking a row acts on that row, so the action has to run against
    /// the clicked workspace even when another one holds focus.
    #[test]
    fn the_clicked_workspace_is_the_one_the_action_runs_against() {
        let mut app = test_app();
        app.state.workspaces = vec![
            crate::workspace::Workspace::test_new("focused"),
            crate::workspace::Workspace::test_new("clicked"),
        ];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        app.state.selected = 0;

        let context = app.context_menu_plugin_context(1);

        assert_eq!(context.workspace_label.as_deref(), Some("clicked"));
        assert_eq!(context.invocation_source.as_deref(), Some("context_menu"));
    }

    #[test]
    fn picking_a_plugin_item_starts_its_command_and_closes_the_menu() {
        let mut app = test_app();
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("repo")];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        let mut plugin = plugin(
            "example.menu",
            vec![action(
                "from-issue",
                "Worktree: from an issue",
                vec![PluginActionContext::Workspace],
            )],
        );
        plugin.actions[0].command = noop_command();
        app.state.install_test_plugins(vec![plugin]);
        let menu = crate::app::state::ContextMenuState {
            kind: ContextMenuKind::Workspace { ws_idx: 0 },
            x: 0,
            y: 0,
            list: crate::app::state::MenuListState::new(0),
            plugin_items: vec![menu_item()],
            hidden_builtins: Vec::new(),
        };
        let plugin_idx = menu.builtin_items().len();
        app.state.mode = crate::app::Mode::ContextMenu;

        app.apply_context_menu_action_via_api(menu, plugin_idx);

        // Starting a command creates the plugin's user directories for real.
        let _ = std::fs::remove_dir_all(crate::plugin_paths::plugin_config_dir("example.menu"));
        let _ = std::fs::remove_dir_all(crate::plugin_paths::plugin_state_dir("example.menu"));

        let log = app
            .state
            .plugin_command_logs
            .last()
            .expect("the pick should have started a plugin command");
        assert_eq!(log.plugin_id, "example.menu");
        assert_eq!(log.action_id.as_deref(), Some("from-issue"));
        assert!(app.state.toast.is_none(), "a successful run says nothing");
        assert_ne!(app.state.mode, crate::app::Mode::ContextMenu);
    }

    #[test]
    fn a_pick_that_cannot_run_leaves_a_toast_instead() {
        let mut app = test_app();
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("repo")];
        app.state.ensure_test_terminals();
        app.state.active = Some(0);
        // The registry is empty, so the menu is naming an action that has since
        // been unlinked — the same shape as a stale menu.
        let menu = crate::app::state::ContextMenuState {
            kind: ContextMenuKind::Workspace { ws_idx: 0 },
            x: 0,
            y: 0,
            list: crate::app::state::MenuListState::new(0),
            plugin_items: vec![menu_item()],
            hidden_builtins: Vec::new(),
        };
        let plugin_idx = menu.builtin_items().len();
        app.state.mode = crate::app::Mode::ContextMenu;

        app.apply_context_menu_action_via_api(menu, plugin_idx);

        assert!(app.state.plugin_command_logs.is_empty());
        let toast = app.state.toast.as_ref().expect("a failure should be shown");
        assert!(
            toast.title.contains("example.menu.from-issue"),
            "the toast should name the action: {toast:?}"
        );
        assert_ne!(app.state.mode, crate::app::Mode::ContextMenu);
    }

    /// The two settings are orthogonal: the mode says which workspaces get a
    /// plugin section at all, the list says which actions go in it.
    #[test]
    fn none_hides_plugin_actions_even_when_an_allowlist_names_them() {
        let mut state = state_with(vec![plugin(
            "worktrunk",
            vec![action("open", "Open", vec![PluginActionContext::Workspace])],
        )]);
        state.plugin_workspace_menu = WorkspaceMenuConfig::None;
        state.plugin_workspace_menu_actions = vec!["worktrunk.open".into()];

        assert!(state.workspace_menu_plugins(true).items.is_empty());
    }

    /// A plugin that supersedes a built-in says so, and Herdr drops the entry
    /// rather than offering two ways to do the same thing.
    #[test]
    fn a_listed_action_hides_the_builtin_it_replaces() {
        let state = state_with(vec![plugin(
            "worktrunk",
            vec![replacing_action(
                "open",
                "Worktree: switch / create",
                vec!["new_worktree", "open_worktree"],
            )],
        )]);

        let plugins = state.workspace_menu_plugins(true);

        assert_eq!(
            plugins.hidden_builtins,
            ["New worktree", "Open worktree..."]
        );
    }

    /// The claim rides with the action: filter the action out and the built-in
    /// comes back, rather than leaving no way to make a worktree at all.
    #[test]
    fn an_allowlist_that_drops_the_action_keeps_the_builtin() {
        let mut state = state_with(vec![plugin(
            "worktrunk",
            vec![
                replacing_action("open", "Worktree: switch / create", vec!["new_worktree"]),
                action(
                    "from-issue",
                    "Worktree: from an issue",
                    vec![PluginActionContext::Workspace],
                ),
            ],
        )]);
        state.plugin_workspace_menu_actions = vec!["worktrunk.from-issue".into()];

        let plugins = state.workspace_menu_plugins(true);

        assert_eq!(titles(&state), ["Worktree: from an issue"]);
        assert!(plugins.hidden_builtins.is_empty());
    }

    /// An id Herdr does not know hides nothing — the manifest warns about it.
    #[test]
    fn an_unknown_replaces_id_hides_nothing() {
        let state = state_with(vec![plugin(
            "worktrunk",
            vec![replacing_action("open", "Open", vec!["rename_workspace"])],
        )]);

        assert!(state
            .workspace_menu_plugins(true)
            .hidden_builtins
            .is_empty());
    }

    /// Nothing is hidden where nothing is offered.
    #[test]
    fn a_plain_directory_keeps_its_builtins() {
        let state = state_with(vec![plugin(
            "worktrunk",
            vec![replacing_action("open", "Open", vec!["new_worktree"])],
        )]);

        let plugins = state.workspace_menu_plugins(false);

        assert!(plugins.items.is_empty());
        assert!(plugins.hidden_builtins.is_empty());
    }

    /// The default: a plain directory has nothing for a worktree action to do.
    #[test]
    fn plugin_actions_are_offered_on_git_workspaces_only_by_default() {
        let state = state_with(vec![plugin(
            "worktrunk",
            vec![action("open", "Open", vec![PluginActionContext::Workspace])],
        )]);
        assert_eq!(state.plugin_workspace_menu, WorkspaceMenuConfig::Git);

        assert_eq!(titles_for(&state, true), ["Open"]);
        assert!(titles_for(&state, false).is_empty());
    }

    #[test]
    fn all_offers_them_on_a_plain_directory_too() {
        let mut state = state_with(vec![plugin(
            "worktrunk",
            vec![action("open", "Open", vec![PluginActionContext::Workspace])],
        )]);
        state.plugin_workspace_menu = WorkspaceMenuConfig::All;

        assert_eq!(titles_for(&state, false), ["Open"]);
    }
}
