use super::*;

#[test]
fn tab_overflow_controls_scroll_the_client_owned_tab_bar() {
    let mut snapshot = snapshot();
    snapshot.tabs.extend((2..=8).map(|number| ClientShellTab {
        tab_id: format!("tab_{number}"),
        workspace_id: "ws_1".into(),
        number,
        label: number.to_string(),
        custom_label: false,
        zoomed: false,
        focused: false,
        agent_status: AgentStatus::Idle,
    }));
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(80, 20).expect("overflow tab bar");

    assert!(state.hits.tab_scroll_right.width > 0);
    let scroll_right = state.hits.tab_scroll_right;
    let outcome =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scroll_right.x + 1,
            row: scroll_right.y,
            modifiers: KeyModifiers::empty(),
        })]);
    assert!(outcome.repaint);
    assert_eq!(state.tab_scroll, 1);

    let mut update = state.snapshot.as_deref().expect("snapshot").clone();
    update.focused_tab_id = Some("tab_8".into());
    for tab in &mut update.tabs {
        tab.focused = tab.tab_id == "tab_8";
    }
    state.set_snapshot(Box::new(update));
    state.compose(80, 20).expect("focused overflow tab");
    assert!(state.hits.tabs.iter().any(|(_, tab_id)| tab_id == "tab_8"));

    state.compose(300, 20).expect("tabs without overflow");
    assert_eq!(state.tab_scroll, 0);
    assert_eq!(state.hits.tabs.len(), 8);
    state.compose(80, 20).expect("focused tab after narrowing");
    assert!(state.hits.tabs.iter().any(|(_, tab_id)| tab_id == "tab_8"));
}

#[test]
fn client_owned_sidebar_dividers_resize_live() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("expanded sidebar");
    let workspace_body = state.hits.workspace_body;
    let needless_scroll =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: workspace_body.x,
            row: workspace_body.y,
            modifiers: KeyModifiers::empty(),
        })]);
    assert_eq!(state.hits.workspace_max_scroll, 0);
    assert_eq!(state.workspace_scroll, 0);
    assert!(!needless_scroll.repaint);
    let width_divider = state.hits.sidebar_divider;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: width_divider.x,
        row: width_divider.y + 2,
        modifiers: KeyModifiers::empty(),
    })]);
    let resize =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 31,
            row: width_divider.y + 2,
            modifiers: KeyModifiers::empty(),
        })]);
    assert_eq!(state.sidebar_width, 32);
    assert!(state.sidebar_width_manual);
    assert!(resize.repaint);
    assert!(resize.resize);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 31,
        row: width_divider.y + 2,
        modifiers: KeyModifiers::empty(),
    })]);

    state.set_pane_surface(surface());
    state.compose(106, 30).expect("resized sidebar");
    let section_divider = state.hits.sidebar_section_divider;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: section_divider.x + 2,
        row: section_divider.y,
        modifiers: KeyModifiers::empty(),
    })]);
    let split = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: section_divider.x + 2,
        row: 20,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(state.sidebar_section_split > 0.6);
    assert!(split.repaint);
    assert!(!split.resize);
}

#[test]
fn context_menus_capture_stable_targets_and_route_actions() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("composed frame");

    let workspace = state.hits.workspaces[0].rect;
    let open_workspace_menu =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: workspace.x + 2,
            row: workspace.y,
            modifiers: KeyModifiers::empty(),
        })]);
    // Opening a workspace menu also asks the endpoint for its plugin actions,
    // so the next open draws them.
    assert!(matches!(
        open_workspace_menu.actions.as_slice(),
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                request.method,
                crate::api::schema::Method::PluginWorkspaceMenu(_)
            )
    ));
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Workspace { ref workspace_id, .. },
            ..
        })) if workspace_id == "ws_1"
    ));
    let workspace_items = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu.items(),
        _ => panic!("workspace context menu"),
    };
    assert!(workspace_items
        .iter()
        .any(|item| item.action == ClientContextMenuAction::NewWorktree));
    state.compose(106, 20).expect("workspace context menu");
    let rename = state.hits.context_menu_rows[0].0;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rename.x + 1,
        row: rename.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            target: ClientRenameTarget::Workspace { ref workspace_id },
            ..
        })) if workspace_id == "ws_1"
    ));

    state.overlay = None;
    state.compose(106, 20).expect("composed frame");
    let pane = state.hits.panes[0].rect;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: pane.x + 1,
        row: pane.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.compose(106, 20).expect("pane context menu");
    let split_index = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu
            .items()
            .iter()
            .position(|item| item.action == ClientContextMenuAction::SplitRight)
            .expect("split right item"),
        _ => panic!("pane context menu"),
    };
    let split = state.hits.context_menu_rows[split_index].0;
    let outcome =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: split.x + 1,
            row: split.y,
            modifiers: KeyModifiers::empty(),
        })]);
    let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
        panic!("pane split context action should use endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::PaneSplit(params)
            if params.target_pane_id.as_deref() == Some("pane_1")
                && params.direction == crate::api::schema::SplitDirection::Right
    ));
}

#[test]
fn global_menu_opens_from_sidebar_and_routes_client_actions() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("shell frame");
    let launcher = state.hits.global_launcher;
    assert_ne!(launcher, Rect::default());

    let open = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: launcher.x,
        row: launcher.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(open.repaint);
    let menu = state.compose(106, 30).expect("global menu");
    let text = menu
        .cells
        .chunks(menu.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("settings"));
    assert!(text.contains("keybinds"));
    assert!(text.contains("reload config"));
    assert!(text.contains("detach"));

    let keybinds = state.hits.global_menu_rows[1].0;
    let help = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: keybinds.x,
        row: keybinds.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(help.actions.is_empty());
    assert!(matches!(state.overlay, Some(ClientShellOverlay::Help(_))));

    state.overlay = Some(ClientShellOverlay::GlobalMenu(ClientGlobalMenuOverlay {
        highlighted: 3,
    }));
    let detach = state.handle_input_bytes(b"\r");
    assert!(detach.detach);
    assert!(state.overlay.is_none());
}

#[test]
fn new_tab_overlay_owns_text_cursor_and_submits_public_api_request() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let mut open = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::NewTab),
        &mut open,
    );
    assert!(open.actions.is_empty());
    let frame = state.compose(106, 20).expect("new tab overlay");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("new tab"));
    assert!(text.contains("save"));
    let restored = frame.to_ratatui_buffer().expect("overlay frame");
    assert!(!restored
        .cell((26, 7))
        .expect("overlay title cell")
        .modifier
        .contains(Modifier::DIM));
    assert!(frame.cursor.as_ref().is_some_and(|cursor| cursor.visible));

    assert!(state.handle_input_bytes(b"logs").actions.is_empty());
    let create = state.handle_input_bytes(b"\r");
    let [ClientShellAction::Endpoint { request, .. }] = &create.actions[..] else {
        panic!("new tab save should use endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::TabCreate(params)
            if params.workspace_id.as_deref() == Some("ws_1")
                && params.label.as_deref() == Some("logs")
    ));
    assert!(state.overlay.is_none());
}

#[test]
fn close_confirmation_error_becomes_client_owned_overlay_and_stable_group_close() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let mut close = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::ClosePane),
        &mut close,
    );
    let [ClientShellAction::Endpoint { request, .. }] = &close.actions[..] else {
        panic!("pane close should use endpoint API");
    };
    let request_id = request.id.clone();
    assert!(
        state
            .handle_endpoint_result(
                "boot-1",
                &request_id,
                Err(ClientShellEndpointError {
                    code: Some("confirmation_required".into()),
                    message: "confirmation required".into(),
                }),
            )
            .0
    );
    let frame = state.compose(106, 20).expect("confirmation overlay");
    let text = frame
        .cells
        .chunks(frame.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Close workspace?"));
    assert!(text.contains("1 pane"));

    let confirm = state.handle_input_bytes(b"\r");
    let [ClientShellAction::Endpoint { request, .. }] = &confirm.actions[..] else {
        panic!("confirmation should use endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::WorkspaceClose(params)
            if params.workspace_id == "ws_1" && params.close_group
    ));
}

/// Herdr's own items keep the top of the menu; plugin actions follow, and a
/// click maps back to the action that was drawn on that row.
#[test]
fn plugin_actions_follow_the_builtin_items_and_map_back_by_index() {
    use crate::api::schema::{PluginWorkspaceMenuItem, PluginWorkspaceMenuItems};

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.workspace_menu_plugins = Some(ClientWorkspaceMenuPlugins {
        git: PluginWorkspaceMenuItems {
            items: vec![PluginWorkspaceMenuItem {
                plugin_id: "worktrunk".into(),
                action_id: "from-issue".into(),
                title: "Worktree: from an issue".into(),
            }],
            hidden_builtins: Vec::new(),
        },
        plain: PluginWorkspaceMenuItems::default(),
    });

    state.open_workspace_context_menu("ws_1".into(), 0, 0);

    let menu = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu,
        _ => panic!("workspace context menu"),
    };
    let labels = menu
        .items()
        .into_iter()
        .map(|item| item.label.into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            "Rename",
            "Close",
            "New worktree",
            "Open worktree...",
            "Worktree: from an issue"
        ]
    );
    assert_eq!(
        menu.items()[4].action,
        ClientContextMenuAction::Plugin(0),
        "the plugin row maps back to the first plugin action"
    );
    assert_eq!(menu.items()[0].action, ClientContextMenuAction::Rename);
}

/// A plugin action that supersedes a built-in takes its place rather than
/// sitting beside it, and the rows below close up.
#[test]
fn a_replaced_builtin_leaves_the_menu_and_the_indexes_close_up() {
    use crate::api::schema::{PluginWorkspaceMenuItem, PluginWorkspaceMenuItems};

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.workspace_menu_plugins = Some(ClientWorkspaceMenuPlugins {
        git: PluginWorkspaceMenuItems {
            items: vec![PluginWorkspaceMenuItem {
                plugin_id: "worktrunk".into(),
                action_id: "open".into(),
                title: "Worktree: switch / create".into(),
            }],
            hidden_builtins: vec!["New worktree".into(), "Open worktree...".into()],
        },
        plain: PluginWorkspaceMenuItems::default(),
    });

    state.open_workspace_context_menu("ws_1".into(), 0, 0);

    let menu = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu,
        _ => panic!("workspace context menu"),
    };
    let items = menu.items();
    let labels = items
        .iter()
        .map(|item| item.label.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["Rename", "Close", "Worktree: switch / create"]);
    assert_eq!(items[2].action, ClientContextMenuAction::Plugin(0));
    assert!(!items
        .iter()
        .any(|item| item.action == ClientContextMenuAction::NewWorktree));
}

/// A plain directory gets the menu its own resolution names, which by default
/// is no plugin section at all.
#[test]
fn a_workspace_without_git_uses_the_plain_resolution() {
    use crate::api::schema::{PluginWorkspaceMenuItem, PluginWorkspaceMenuItems};

    let mut plain_snapshot = snapshot();
    plain_snapshot.workspaces[0].branch = None;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(plain_snapshot));
    state.workspace_menu_plugins = Some(ClientWorkspaceMenuPlugins {
        git: PluginWorkspaceMenuItems {
            items: vec![PluginWorkspaceMenuItem {
                plugin_id: "worktrunk".into(),
                action_id: "open".into(),
                title: "Worktree: switch / create".into(),
            }],
            hidden_builtins: Vec::new(),
        },
        plain: PluginWorkspaceMenuItems::default(),
    });

    state.open_workspace_context_menu("ws_1".into(), 0, 0);

    let menu = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu,
        _ => panic!("workspace context menu"),
    };
    let labels = menu
        .items()
        .into_iter()
        .map(|item| item.label.into_owned())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["Rename", "Close"]);
}

/// Right-clicking a row acts on that row: the invoke names the clicked
/// workspace, so the endpoint runs the action against it and not against
/// whichever workspace happens to hold focus.
#[test]
fn picking_a_plugin_item_invokes_it_against_the_clicked_workspace() {
    use crate::api::schema::{Method, PluginWorkspaceMenuItem, PluginWorkspaceMenuItems};

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.workspace_menu_plugins = Some(ClientWorkspaceMenuPlugins {
        git: PluginWorkspaceMenuItems {
            items: vec![PluginWorkspaceMenuItem {
                plugin_id: "worktrunk".into(),
                action_id: "from-issue".into(),
                title: "Worktree: from an issue".into(),
            }],
            hidden_builtins: Vec::new(),
        },
        plain: PluginWorkspaceMenuItems::default(),
    });
    state.open_workspace_context_menu("ws_1".into(), 0, 0);
    let plugin_index = match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu
            .items()
            .iter()
            .position(|item| item.action == ClientContextMenuAction::Plugin(0))
            .expect("plugin row"),
        _ => panic!("workspace context menu"),
    };

    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(plugin_index, &mut outcome);

    let invoked = outcome
        .actions
        .iter()
        .find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => match &request.method {
                Method::PluginActionInvoke(params) => Some(params),
                _ => None,
            },
            _ => None,
        })
        .expect("the pick should invoke the plugin action");
    assert_eq!(invoked.plugin_id.as_deref(), Some("worktrunk"));
    assert_eq!(invoked.action_id, "from-issue");
    let context = invoked.context.as_ref().expect("an invocation context");
    assert_eq!(context.workspace_id.as_deref(), Some("ws_1"));
    assert_eq!(context.invocation_source.as_deref(), Some("context_menu"));
    assert!(state.overlay.is_none(), "the menu closes behind the pick");
}

/// Against an endpoint that does not advertise the method there is no plugin
/// section and, importantly, no "action unavailable" notice on every mouse move.
#[test]
fn an_endpoint_without_the_method_gets_no_plugin_section_and_no_notice() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_endpoint_methods(Some(vec!["workspace.close".into()]));

    let mut outcome = ClientShellInput::default();
    state.refresh_workspace_menu_plugins(&mut outcome);

    assert!(outcome.actions.is_empty());
    assert!(state.visible_endpoint_notice.is_none());
    assert!(state.workspace_menu_plugins.is_none());
}
