// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_project_blocked_by_a_native_panel_is_not_remembered() {
    struct Panel;
    impl suhina_session::LiveOutput for Panel {
        fn info(&self) -> suhina_session::OutputInfo {
            Default::default()
        }
        fn control_panel_open(&self) -> bool {
            true
        }
    }
    let path = std::env::temp_dir().join(format!("suhina-blocked-open-{}.suhina", std::process::id()));
    Session::new(Default::default()).save(&path, false).unwrap();
    let mut app = App::new();
    let mut session = Session::new(Default::default());
    let mut renderer = suhina_engine::Renderer::stopped(session.project(), &Default::default(), 48_000).unwrap();
    session.attach_player(suhina_session::Player::with_output(renderer.controller(), None, std::sync::Arc::new(Panel)));
    let active = app.projects_mut().add(session, None).unwrap();
    let mut window = Window {
        app,
        agents: None,
        splash: None,
        start_up: None,
        graphics_now: Graphics::default(),
        editors_embeddable: false,
        see_through: Some(false),
        blurred: false,
        themes: theme_io::ThemeIo::new(theme_files::ThemeFiles::new(None)).unwrap(),
        themes_folder: None,
        data_folder: String::new(),
        theme_error: None,
        recent: recent::Recent::load(None),
        #[cfg(target_os = "macos")]
        mac_menu: None,
    };
    window.open(&path, OpenAs::NewTab);
    assert!(window.recent.existing().is_empty());
    assert_eq!(window.app.projects().active(), Some(active));
    assert_eq!(window.app.projects().len(), 1);
    assert!(matches!(window.app.status(), Status::Error(message) if message.contains("control panel")));
    std::fs::remove_file(path).unwrap();
}
