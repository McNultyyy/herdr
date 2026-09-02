//! Plugin actions offered in the workspace right-click menu.
//!
//! Plugin v1 lets an action declare `contexts = ["workspace"]`, but nothing
//! rendered them. The endpoint resolves them here, per workspace, and puts the
//! result in the client shell snapshot; the client draws them after its own
//! items and dispatches a pick back through `plugin.action.invoke`.

use crate::api::schema::PluginActionContext;
use crate::app::state::AppState;

use super::manifest::{effective_platforms, ensure_platform_supported};
use super::{manifest_action_info, plugin_manifest_available};

/// A plugin action offered in a workspace menu, resolved when the snapshot was
/// built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceMenuPluginItem {
    pub(crate) plugin_id: String,
    pub(crate) action_id: String,
    pub(crate) title: String,
}

impl WorkspaceMenuPluginItem {
    pub(crate) fn qualified_id(&self) -> String {
        format!("{}.{}", self.plugin_id, self.action_id)
    }
}

/// What plugins contribute to one workspace context menu.
#[derive(Debug, Default)]
pub(crate) struct WorkspaceMenuPlugins {
    /// Actions to list after Herdr's own items.
    pub(crate) items: Vec<WorkspaceMenuPluginItem>,
    /// Built-in item labels to leave out, because a listed action replaces them.
    pub(crate) hidden_builtins: Vec<&'static str>,
}

impl AppState {
    /// The plugin actions to append to a workspace context menu, in the order
    /// they should appear.
    ///
    /// Read from the cached registry — loaded at startup and refreshed by every
    /// plugin API call, including `plugin link` — so building a snapshot never
    /// waits on disk. Actions are filtered to the ones that could actually run:
    /// an enabled plugin whose manifest is readable, declaring the workspace
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
        let mut items: Vec<WorkspaceMenuPluginItem> = Vec::new();
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
                let item = WorkspaceMenuPluginItem {
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

#[cfg(test)]
mod tests {
    use crate::api::schema::{InstalledPluginInfo, PluginActionContext, PluginPlatform};
    use crate::app::state::AppState;
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
        let pane_only = plugin(
            "pane-only",
            vec![action("p", "Pane", vec![PluginActionContext::Pane])],
        );

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
