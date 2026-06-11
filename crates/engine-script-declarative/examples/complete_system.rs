//! Complete example showing all 6 declarative systems.

use engine_script_declarative::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Complete Declarative Game Systems Example ===\n");

    // 1. Create a complete game project
    let mut project = ProjectSchema::new("CyberpunkTowerDefense");
    project.description = "赛博朋克风格塔防游戏".to_string();
    project.genre = Some("tower_defense".to_string());
    project.art_style = Some("cyberpunk".to_string());

    // 2. Add scenes
    project.add_scene("MainMenu", "scenes/main_menu.json");
    project.add_scene("Level1", "scenes/level1.json");
    project.default_scene = Some("MainMenu".to_string());

    // 3. Add UI layouts
    project.add_ui("GameHUD", "ui/hud.json", "game");
    project.add_ui("MainMenu", "ui/main_menu.json", "menu");

    // 4. Configure game systems
    project.systems.combat = Some(CombatSystem {
        damage_multiplier: 1.5,
        friendly_fire: false,
        crit_chance: 0.1,
        crit_multiplier: 2.0,
        invincibility_duration: 1.0,
    });

    project.systems.economy = Some(EconomySystem {
        starting_currency: 500,
        currency_name: "Credits".to_string(),
        price_multiplier: 1.0,
        currency_drops: true,
    });

    // 5. Add assets to manifest
    project.assets.add_asset(
        "models",
        AssetRef {
            id: "player_model".to_string(),
            path: "models/player.gltf".to_string(),
            meta: None,
            loading: LoadingStrategy::Preload,
        },
    );

    project.assets.add_asset(
        "textures",
        AssetRef {
            id: "ground_texture".to_string(),
            path: "textures/ground.png".to_string(),
            meta: None,
            loading: LoadingStrategy::Preload,
        },
    );

    // Validate project
    project.validate()?;
    println!("✅ Project schema validated successfully");

    // Serialize to JSON
    let project_json = serde_json::to_string_pretty(&project)?;
    println!("\n📄 Generated Project JSON:\n");
    println!("{}", project_json);

    println!("\n=== Creating Scene (three.js-like) ===\n");

    // 6. Create a scene (three.js style)
    let scene = SceneSchema {
        name: "Level1".to_string(),
        children: vec![
            // Camera
            Object3D::Camera {
                name: "MainCamera".to_string(),
                position: [0.0, 10.0, 20.0],
                rotation: [-0.5, 0.0, 0.0],
                camera_type: CameraType::Perspective {
                    fov: 60.0,
                    near: 0.1,
                    far: 1000.0,
                },
            },
            // Directional light
            Object3D::Light {
                name: "Sun".to_string(),
                position: [10.0, 20.0, 10.0],
                light_type: LightType::Directional,
                color: [1.0, 1.0, 0.9],
                intensity: 1.0,
            },
            // Ground plane
            Object3D::Mesh {
                name: "Ground".to_string(),
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                geometry: GeometryRef::Plane {
                    width: 100.0,
                    height: 100.0,
                },
                material: MaterialRef::Standard {
                    color: [0.3, 0.5, 0.3],
                    metalness: 0.0,
                    roughness: 0.8,
                    map: None,
                },
                children: vec![],
                behavior: None,
            },
            // Enemy group
            Object3D::Group {
                name: "Enemies".to_string(),
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                children: vec![Object3D::Mesh {
                    name: "Enemy1".to_string(),
                    position: [10.0, 1.0, 5.0],
                    rotation: [0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                    geometry: GeometryRef::Box {
                        width: 1.0,
                        height: 2.0,
                        depth: 1.0,
                    },
                    material: MaterialRef::Basic {
                        color: [1.0, 0.0, 0.0],
                        map: None,
                    },
                    children: vec![],
                    behavior: Some("behaviors/enemy_patrol.json".to_string()),
                }],
            },
        ],
        environment: Environment {
            skybox: Some("cyberpunk_night".to_string()),
            ambient_light: [0.2, 0.2, 0.3],
            fog: Some(FogConfig {
                density: 0.01,
                color: [0.1, 0.1, 0.2],
            }),
        },
    };

    scene.validate()?;
    println!("✅ Scene schema validated successfully");

    let scene_json = serde_json::to_string_pretty(&scene)?;
    println!("\n📄 Generated Scene JSON (three.js-like):\n");
    println!("{}", &scene_json[..500.min(scene_json.len())]);
    println!("... (truncated)");

    println!("\n=== Creating UI Layout ===\n");

    // 7. Create UI (HUD)
    let ui = UISchema {
        name: "GameHUD".to_string(),
        layout: LayoutType::Anchored,
        elements: vec![
            UIElement::Bar {
                binding: "player.health".to_string(),
                style: BarStyle::default(),
                position: Position {
                    anchor: Anchor::TopLeft,
                    offset: [10.0, 10.0],
                },
            },
            UIElement::Text {
                content: "Tower Defense".to_string(),
                style: TextStyle::default(),
                position: Position {
                    anchor: Anchor::TopCenter,
                    offset: [0.0, 10.0],
                },
            },
            UIElement::Button {
                text: "Pause".to_string(),
                action: "pause_game".to_string(),
                style: ButtonStyle::default(),
                position: Position {
                    anchor: Anchor::TopRight,
                    offset: [-10.0, 10.0],
                },
            },
        ],
        bindings: vec![DataBinding {
            id: "player.health".to_string(),
            source: "entities.player.components.health.current".to_string(),
        }],
    };

    ui.validate()?;
    println!("✅ UI schema validated successfully");

    let ui_json = serde_json::to_string_pretty(&ui)?;
    println!("\n📄 Generated UI JSON:\n");
    println!("{}", ui_json);

    println!("\n=== Summary ===\n");
    println!("✅ All 6 declarative systems working:");
    println!("   1. Behavior Trees - For game logic (see previous examples)");
    println!("   2. Scene Graphs - three.js-like 3D hierarchy ✅");
    println!("   3. UI Layouts - Declarative interfaces ✅");
    println!("   4. Systems Config - Combat, economy, etc ✅");
    println!("   5. Asset Manifest - Resource management ✅");
    println!("   6. Project Structure - Complete game definition ✅");
    println!("\n🚀 Ready for AI agents to generate complete games!");

    Ok(())
}
