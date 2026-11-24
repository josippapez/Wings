use std::path::Path;

use gpui::{App, Bounds, DisplayId, Pixels, PlatformDisplay, WindowBounds, point, px, size};

use crate::saved::SavedWindowBounds;

const DEFAULT_SIZE: (f32, f32) = (1000.0, 640.0);
const MIN_SIZE: (f32, f32) = (640.0, 320.0);

pub struct Restored {
    pub bounds: WindowBounds,
    pub display_id: Option<DisplayId>,
}

pub fn restore(state: Option<&Path>, cx: &App) -> Restored {
    let default = WindowBounds::centered(None, size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)), cx);
    let saved = state.and_then(|state| crate::saved::load_settings(state).window_bounds);
    restore_saved(saved.as_ref(), cx, default)
}

fn restore_saved(saved: Option<&SavedWindowBounds>, cx: &App, default: WindowBounds) -> Restored {
    let Some(saved) = saved else {
        return fallback(default);
    };
    let Some(uuid) = saved_display(saved) else {
        return fallback(default);
    };
    restore_from_displays(
        saved,
        uuid,
        cx.displays().iter().filter_map(|display| {
            display.uuid().ok().map(|uuid| (uuid.to_string(), display.id(), display.visible_bounds()))
        }),
        default,
    )
}

fn restore_from_displays(
    saved: &SavedWindowBounds,
    saved_uuid: &str,
    mut displays: impl Iterator<Item = (String, DisplayId, Bounds<Pixels>)>,
    default: WindowBounds,
) -> Restored {
    let Some((_, display_id, visible_bounds)) = displays.find(|(uuid, _, _)| uuid == saved_uuid) else {
        return fallback(default);
    };
    let Some(bounds) = restore_on_display(saved, visible_bounds) else {
        return fallback(default);
    };
    Restored {
        bounds,
        display_id: Some(display_id),
    }
}

fn fallback(bounds: WindowBounds) -> Restored {
    Restored {
        bounds,
        display_id: None,
    }
}

pub fn save(bounds: WindowBounds, display: Option<&dyn PlatformDisplay>) -> SavedWindowBounds {
    let rectangle = bounds.get_bounds();
    let display = display.and_then(|display| display.uuid().ok()).map(|uuid| uuid.to_string());
    let (x, y, width, height) = (
        rectangle.origin.x.as_f32(),
        rectangle.origin.y.as_f32(),
        rectangle.size.width.as_f32(),
        rectangle.size.height.as_f32(),
    );
    match bounds {
        WindowBounds::Windowed(_) => SavedWindowBounds::Windowed {
            display,
            x,
            y,
            width,
            height,
        },
        WindowBounds::Maximized(_) => SavedWindowBounds::Maximized {
            display,
            x,
            y,
            width,
            height,
        },
        WindowBounds::Fullscreen(_) => SavedWindowBounds::Fullscreen {
            display,
            x,
            y,
            width,
            height,
        },
    }
}

fn restore_on_display(
    saved: &SavedWindowBounds,
    display: Bounds<Pixels>,
) -> Option<WindowBounds> {
    let (kind, mut bounds) = saved_parts(saved);
    if ![bounds.origin.x, bounds.origin.y, bounds.size.width, bounds.size.height]
        .into_iter()
        .all(|value| value.as_f32().is_finite())
        || bounds.size.width < px(MIN_SIZE.0)
        || bounds.size.height < px(MIN_SIZE.1)
        || overlap(bounds, display) <= 0.0
    {
        return None;
    }
    bounds.size.width = bounds.size.width.min(display.size.width);
    bounds.size.height = bounds.size.height.min(display.size.height);
    bounds.origin.x = bounds.origin.x.clamp(
        display.origin.x,
        display.origin.x + display.size.width - bounds.size.width,
    );
    bounds.origin.y = bounds.origin.y.clamp(
        display.origin.y,
        display.origin.y + display.size.height - bounds.size.height,
    );
    match kind {
        WindowKind::Windowed => Some(WindowBounds::Windowed(bounds)),
        WindowKind::Maximized => Some(WindowBounds::Maximized(bounds)),
        WindowKind::Fullscreen => Some(WindowBounds::Fullscreen(bounds)),
    }
}

fn overlap(a: Bounds<Pixels>, b: Bounds<Pixels>) -> f32 {
    let width = (a.origin.x + a.size.width)
        .min(b.origin.x + b.size.width)
        .as_f32()
        - a.origin.x.max(b.origin.x).as_f32();
    let height = (a.origin.y + a.size.height)
        .min(b.origin.y + b.size.height)
        .as_f32()
        - a.origin.y.max(b.origin.y).as_f32();
    width.max(0.0) * height.max(0.0)
}

#[derive(Clone, Copy)]
enum WindowKind {
    Windowed,
    Maximized,
    Fullscreen,
}

fn saved_display(saved: &SavedWindowBounds) -> Option<&str> {
    match saved {
        SavedWindowBounds::Windowed { display, .. }
        | SavedWindowBounds::Maximized { display, .. }
        | SavedWindowBounds::Fullscreen { display, .. } => display.as_deref(),
    }
}

fn saved_parts(saved: &SavedWindowBounds) -> (WindowKind, Bounds<Pixels>) {
    let (kind, x, y, width, height) = match saved {
        SavedWindowBounds::Windowed {
            x,
            y,
            width,
            height,
            ..
        } => (WindowKind::Windowed, x, y, width, height),
        SavedWindowBounds::Maximized {
            x,
            y,
            width,
            height,
            ..
        } => (WindowKind::Maximized, x, y, width, height),
        SavedWindowBounds::Fullscreen {
            x,
            y,
            width,
            height,
            ..
        } => (WindowKind::Fullscreen, x, y, width, height),
    };
    (kind, Bounds::new(point(px(*x), px(*y)), size(px(*width), px(*height))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "test-support")]
    use gpui::TestApp;

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn invalid_rectangles_use_the_centered_default() {
        let display = bounds(0.0, 0.0, 1920.0, 1080.0);
        for saved in [
            SavedWindowBounds::Windowed { display: Some("secondary".into()), x: 0.0, y: 0.0, width: 639.0, height: 640.0 },
            SavedWindowBounds::Windowed { display: Some("secondary".into()), x: 0.0, y: 0.0, width: 640.0, height: 319.0 },
            SavedWindowBounds::Windowed { display: Some("secondary".into()), x: f32::NAN, y: 0.0, width: 640.0, height: 320.0 },
            SavedWindowBounds::Windowed { display: Some("secondary".into()), x: 3000.0, y: 0.0, width: 640.0, height: 320.0 },
        ] {
            assert_eq!(restore_on_display(&saved, display), None);
        }
    }

    #[test]
    fn clamps_oversized_rectangles_in_display_local_coordinates() {
        let display = bounds(-1600.0, 0.0, 1600.0, 900.0);
        let saved = SavedWindowBounds::Maximized {
            display: Some("secondary".into()),
            x: -1500.0,
            y: -50.0,
            width: 2000.0,
            height: 1000.0,
        };
        assert_eq!(
            restore_on_display(&saved, display),
            Some(WindowBounds::Maximized(bounds(-1600.0, 0.0, 1600.0, 900.0)))
        );
    }

    #[test]
    fn restores_on_the_saved_display_even_when_monitors_share_an_origin() {
        let default = WindowBounds::Windowed(bounds(0.0, 0.0, 1000.0, 640.0));
        let saved = SavedWindowBounds::Fullscreen {
            display: Some("second".into()),
            x: 40.0,
            y: 30.0,
            width: 900.0,
            height: 700.0,
        };
        let restored = restore_from_displays(
            &saved,
            saved_display(&saved).unwrap(),
            [
                ("first".into(), DisplayId::new(1), bounds(0.0, 0.0, 1920.0, 1080.0)),
                ("second".into(), DisplayId::new(2), bounds(0.0, 0.0, 1200.0, 800.0)),
            ]
            .into_iter(),
            default,
        );
        assert_eq!(restored.display_id, Some(DisplayId::new(2)));
        assert_eq!(restored.bounds, WindowBounds::Fullscreen(bounds(40.0, 30.0, 900.0, 700.0)));

        let missing = restore_from_displays(
            &saved,
            "missing",
            std::iter::once((
                "primary".into(),
                DisplayId::new(1),
                bounds(0.0, 0.0, 1920.0, 1080.0),
            )),
            default,
        );
        assert_eq!(missing.display_id, None);
        assert_eq!(missing.bounds, default);
    }

    #[test]
    fn invalid_saved_geometry_on_a_connected_secondary_falls_back_to_primary() {
        let default = WindowBounds::Windowed(bounds(100.0, 50.0, 1000.0, 640.0));
        let saved = SavedWindowBounds::Windowed {
            display: Some("secondary".into()),
            x: 3000.0,
            y: 0.0,
            width: 640.0,
            height: 320.0,
        };
        let restored = restore_from_displays(
            &saved,
            "secondary",
            std::iter::once((
                "secondary".into(),
                DisplayId::new(2),
                bounds(0.0, 0.0, 1200.0, 800.0),
            )),
            default,
        );
        assert_eq!(restored.display_id, None);
        assert_eq!(restored.bounds, default);
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn first_launch_uses_the_centered_default() {
        let app = TestApp::new();
        app.read(|cx| {
            let restored = restore(None, cx);
            assert_eq!(restored.display_id, None);
            assert_eq!(
                restored.bounds,
                WindowBounds::centered(None, size(px(1000.0), px(640.0)), cx)
            );
        });
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn restores_a_valid_rectangle_from_the_saved_workspace_file() {
        let path = std::env::temp_dir().join(format!(
            "wings-native-window-state-restore-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let app = TestApp::new();
        app.read(|cx| {
            let display = cx.primary_display().unwrap();
            crate::saved::write_all(
                &path,
                crate::saved::Settings {
                    window_bounds: Some(SavedWindowBounds::Windowed {
                        display: Some(display.uuid().unwrap().to_string()),
                        x: 100.0,
                        y: 100.0,
                        width: 800.0,
                        height: 600.0,
                    }),
                    ..Default::default()
                },
                Default::default(),
            )
            .unwrap();
            let restored = restore(Some(&path), cx);
            assert_eq!(restored.display_id, Some(display.id()));
            assert_eq!(
                restored.bounds,
                WindowBounds::Windowed(bounds(100.0, 100.0, 800.0, 600.0))
            );
        });
        std::fs::remove_file(path).unwrap();
    }
}
