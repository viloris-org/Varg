/// Physics backend integration: null backend satisfies the full contract.
#[cfg(feature = "physics")]
mod physics_tests {
    use engine_physics::{
        ColliderDesc, LayerMatrix, NullPhysicsBackend, PhysicsBackend, PhysicsWorld, QueryFilter,
        RigidbodyDesc, Vec3,
    };

    #[test]
    fn physics_world_null_backend_fixed_update_does_not_panic() {
        let mut world = PhysicsWorld::null();
        world.fixed_update(1.0 / 60.0);
    }

    #[test]
    fn physics_world_null_backend_raycast_returns_none() {
        let world = PhysicsWorld::null();
        assert!(world
            .backend()
            .raycast(
                Vec3::ZERO,
                Vec3::new(0.0, 0.0, 1.0),
                100.0,
                QueryFilter::default()
            )
            .is_none());
    }

    #[test]
    fn physics_world_null_backend_overlap_returns_empty() {
        let world = PhysicsWorld::null();
        assert!(world
            .backend()
            .overlap_sphere(Vec3::ZERO, 1.0, QueryFilter::default())
            .is_empty());
    }

    #[test]
    fn physics_world_null_backend_contacts_are_empty() {
        let mut world = PhysicsWorld::null();
        assert!(world.backend_mut().drain_contacts().is_empty());
    }

    #[test]
    fn layer_matrix_symmetric_disable() {
        let mut matrix = LayerMatrix::default();
        assert!(matrix.collides(0, 1));
        matrix.set(0, 1, false);
        assert!(!matrix.collides(0, 1));
        assert!(!matrix.collides(1, 0));
    }

    #[test]
    fn collider_desc_defaults_are_sensible() {
        let desc = ColliderDesc::default();
        assert!(!desc.is_trigger);
        assert_eq!(desc.friction, 0.5);
    }

    #[test]
    fn rigidbody_desc_default_is_dynamic() {
        use engine_physics::BodyKind;
        assert_eq!(RigidbodyDesc::default().kind, BodyKind::Dynamic);
    }

    #[test]
    fn null_backend_create_body_returns_error() {
        let mut backend = NullPhysicsBackend;
        assert!(backend.create_body(&RigidbodyDesc::default()).is_err());
    }
}

/// Audio backend integration: null backend satisfies the full contract.
#[cfg(feature = "audio")]
mod audio_tests {
    use engine_audio::{
        AudioContext, AudioListenerDesc, AudioSourceDesc, ClipHandle, NullAudioBackend,
        SourceHandle, Vec3,
    };

    #[test]
    fn audio_context_null_backend_update_does_not_panic() {
        let mut ctx = AudioContext::null();
        ctx.update(1.0 / 60.0);
    }

    #[test]
    fn audio_context_null_backend_load_clip_returns_error() {
        let mut ctx = AudioContext::null();
        assert!(ctx.backend_mut().load_clip("test", &[], 1, 44100).is_err());
    }

    #[test]
    fn audio_context_null_backend_play_pause_stop_are_noops() {
        let mut ctx = AudioContext::null();
        let handle = SourceHandle(0);
        assert!(ctx.backend_mut().play(handle).is_ok());
        assert!(ctx.backend_mut().pause(handle).is_ok());
        assert!(ctx.backend_mut().stop(handle).is_ok());
    }

    #[test]
    fn audio_source_desc_simple_defaults() {
        let desc = AudioSourceDesc::simple(ClipHandle(1));
        assert_eq!(desc.volume, 1.0);
        assert!(!desc.looping);
        assert!(desc.auto_play);
    }

    #[test]
    fn audio_listener_default_faces_negative_z() {
        let listener = AudioListenerDesc::default();
        assert_eq!(listener.forward, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn null_backend_playback_state_returns_error() {
        use engine_audio::AudioBackend;
        let backend = NullAudioBackend;
        assert!(backend.playback_state(SourceHandle(0)).is_err());
    }
}

/// Editor panel and service integration.
#[cfg(feature = "editor")]
mod editor_tests {
    use engine_editor::{
        register_core_commands, register_core_panels, CommandRegistry, ConsoleEntry, ConsoleFilter,
        ConsoleLevel, ConsoleService, ConsoleSource, EditorPreferences, PanelRegistry, Selection,
        SelectionService,
    };
    use engine_editor_ui::{EditorShell, HubPage, HubState};

    #[test]
    fn core_panels_are_registered() {
        let mut registry = PanelRegistry::default();
        register_core_panels(&mut registry);
        for id in [
            "hierarchy",
            "inspector",
            "project",
            "console",
            "scene_view",
            "game_view",
        ] {
            assert!(registry.get(id).is_some(), "missing panel: {id}");
        }
    }

    #[test]
    fn core_commands_are_registered() {
        let mut registry = CommandRegistry::default();
        register_core_commands(&mut registry);
        for id in [
            "play.toggle",
            "play.pause",
            "play.stop",
            "assets.reload",
            "scene.save",
            "project.build",
        ] {
            assert!(registry.get(id).is_some(), "missing command: {id}");
        }
    }

    #[test]
    fn editor_shell_opens_with_core_services() {
        let shell = EditorShell::with_core_services(EditorPreferences::default());
        assert!(shell.panels().get("scene_view").is_some());
        assert!(shell.panels().get("game_view").is_some());
        assert!(shell.commands().get("play.toggle").is_some());
    }

    #[test]
    fn hub_state_starts_on_projects_page() {
        let hub = HubState::new(EditorPreferences::default());
        assert_eq!(hub.page(), HubPage::Projects);
    }

    #[test]
    fn selection_service_select_and_clear() {
        let mut svc = SelectionService::default();
        svc.select(Selection::Entity("player".into()));
        assert!(svc.selected().is_some());
        svc.clear();
        assert!(svc.selected().is_none());
    }

    #[test]
    fn console_service_filter_by_level() {
        let mut console = ConsoleService::default();
        console.push(ConsoleEntry {
            timestamp: "t0".into(),
            level: ConsoleLevel::Info,
            source: ConsoleSource {
                subsystem: "sys".into(),
                file: None,
                line: None,
            },
            message: "info msg".into(),
        });
        console.push(ConsoleEntry {
            timestamp: "t1".into(),
            level: ConsoleLevel::Error,
            source: ConsoleSource {
                subsystem: "sys".into(),
                file: None,
                line: None,
            },
            message: "error msg".into(),
        });
        let errors = console.filtered(&ConsoleFilter {
            min_level: Some(ConsoleLevel::Error),
            source_contains: None,
            message_contains: None,
        });
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].level, ConsoleLevel::Error);
    }
}
