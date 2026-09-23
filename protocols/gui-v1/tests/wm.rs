//! Pure window-management + compositor model (ported from the
//! `userspace/display_server` proptest suite, minus egui). The originals used
//! `proptest`, which is unavailable offline, so each property is re-expressed
//! as a deterministic sweep over representative and edge-case inputs.

use gui_protocol_v1::wm::{WindowManager, COLOR_CURSOR, MOUSE_LEFT};

/// Ported from `window_creation_properties::prop_window_creation_allocates_resources`.
#[test]
fn window_creation_allocates_resources() {
    for &pid in &[1u32, 42, 999] {
        for &x in &[-1000i32, 0, 777] {
            for &y in &[-1000i32, 0, 512] {
                for &(w, h) in &[(1u32, 1u32), (200, 200), (1999, 1999)] {
                    for &buf in &[1u64, 9999] {
                        let mut wm = WindowManager::new();
                        let id = wm.create_window(pid, x, y, w, h, buf);
                        let win = wm.get_window(id).expect("window exists");
                        assert_eq!(win.id, id);
                        assert_eq!(win.owner_pid, pid);
                        assert_eq!((win.x, win.y), (x, y));
                        assert_eq!((win.width, win.height), (w, h));
                        assert_eq!(win.buffer, buf);
                        assert!(win.visible);
                        assert_eq!(wm.window_count(), 1);
                    }
                }
            }
        }
    }
}

/// Ported from `window_management_properties::prop_window_position_updates_on_drag`.
#[test]
fn window_position_updates_on_drag() {
    for &ix in &[-500i32, 0, 500] {
        for &iy in &[-500i32, 0, 500] {
            for &dsx in &[0i32, 50, 99] {
                for &dsy in &[0i32, 50, 99] {
                    for &dex in &[0i32, 50, 99] {
                        for &dey in &[0i32, 50, 99] {
                            let mut wm = WindowManager::new();
                            let id = wm.create_window(1, ix, iy, 200, 200, 1);
                            // Press inside the body, then move with the button held.
                            wm.handle_mouse(ix + dsx, iy + dsy, MOUSE_LEFT);
                            wm.handle_mouse(ix + dex, iy + dey, MOUSE_LEFT);
                            let win = wm.get_window(id).unwrap();
                            assert_eq!(win.x, (ix + dex) - dsx, "x after drag");
                            assert_eq!(win.y, (iy + dey) - dsy, "y after drag");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn release_ends_drag() {
    let mut wm = WindowManager::new();
    let id = wm.create_window(1, 0, 0, 200, 200, 1);
    wm.handle_mouse(10, 10, MOUSE_LEFT); // press, begin drag
    wm.handle_mouse(60, 60, MOUSE_LEFT); // drag -> window at (50,50)
    assert_eq!((wm.get_window(id).unwrap().x, wm.get_window(id).unwrap().y), (50, 50));
    wm.handle_mouse(60, 60, 0); // release
    assert!(wm.pointer().dragging.is_none());
    // Moving with no button held must not move the window.
    wm.handle_mouse(500, 500, 0);
    assert_eq!((wm.get_window(id).unwrap().x, wm.get_window(id).unwrap().y), (50, 50));
}

/// Ported from `window_management_properties::prop_window_focus_management`.
#[test]
fn focus_is_exclusive() {
    for num in 2usize..=9 {
        let mut wm = WindowManager::new();
        let mut ids = Vec::new();
        for i in 0..num {
            ids.push(wm.create_window(i as u32 + 1, (i * 100) as i32, (i * 100) as i32, 100, 100, i as u64 + 1));
        }
        for target in 0..num {
            assert!(wm.set_focus(ids[target]));
            assert_eq!(wm.focused_window(), Some(ids[target]));
            for (i, &id) in ids.iter().enumerate() {
                assert_eq!(wm.get_window(id).unwrap().focused, i == target, "window {i} focus");
            }
        }
    }
}

#[test]
fn focus_auto_on_first_and_reassigns_on_destroy() {
    let mut wm = WindowManager::new();
    let a = wm.create_window(1, 0, 0, 100, 100, 1);
    assert_eq!(wm.focused_window(), Some(a), "first window auto-focused");
    let b = wm.create_window(2, 200, 0, 100, 100, 2);
    assert_eq!(wm.focused_window(), Some(a), "second does not steal focus");
    // Destroying the focused window hands focus to the lowest surviving id.
    assert!(wm.destroy_window(a));
    assert_eq!(wm.focused_window(), Some(b));
    assert!(wm.get_window(b).unwrap().focused);
    // Destroying the last window clears focus.
    assert!(wm.destroy_window(b));
    assert_eq!(wm.focused_window(), None);
    assert_eq!(wm.window_count(), 0);
    // Destroying an unknown window is a no-op.
    assert!(!wm.destroy_window(999));
}

#[test]
fn hit_test_prefers_topmost_and_skips_hidden() {
    let mut wm = WindowManager::new();
    let bottom = wm.create_window(1, 0, 0, 100, 100, 1);
    let top = wm.create_window(2, 0, 0, 100, 100, 2); // overlaps bottom, higher id
    // Overlap resolves to the higher-id (top) window.
    assert_eq!(wm.window_at(50, 50), Some(top));
    // Hiding the top exposes the bottom.
    wm.get_window_mut(top).unwrap().visible = false;
    assert_eq!(wm.window_at(50, 50), Some(bottom));
    // A point outside every body hits nothing.
    assert_eq!(wm.window_at(500, 500), None);
    // Title-bar rows (above the body) are not part of the hit rectangle.
    assert_eq!(wm.window_at(50, -1), None);
}

/// Ported from `compositing_properties::prop_compositing_includes_all_visible_windows`.
#[test]
fn compositing_paints_every_visible_window() {
    for num in 1usize..=4 {
        for &(sw, sh) in &[(800u32, 600u32), (1280, 800), (1920, 1080)] {
            let mut wm = WindowManager::new();
            let mut ids = Vec::new();
            for i in 0..num {
                ids.push(wm.create_window(i as u32 + 1, (i * 100) as i32, (i * 100) as i32, 200, 200, i as u64 + 1));
            }
            let fb = wm.render_frame(sw, sh);
            assert_eq!(fb.len(), (sw * sh) as usize);
            for &id in &ids {
                let win = wm.get_window(id).unwrap();
                let (cx, cy) = (win.x + 10, win.y + 10); // interior point
                if cx >= 0 && cx < sw as i32 && cy >= 0 && cy < sh as i32 {
                    let px = fb[(cy as u32 * sw + cx as u32) as usize];
                    assert_ne!(px, 0, "window {id} interior painted");
                }
            }
        }
    }
}

/// Ported from `compositing_properties::prop_compositing_renders_cursor`.
#[test]
fn compositing_paints_cursor_on_top() {
    for &(mx, my) in &[(0i32, 0i32), (400, 300), (799, 599)] {
        for &(sw, sh) in &[(800u32, 600u32), (1920, 1080)] {
            let mut wm = WindowManager::new();
            wm.handle_mouse(mx, my, 0);
            let fb = wm.render_frame(sw, sh);
            if mx >= 0 && mx < sw as i32 && my >= 0 && my < sh as i32 {
                let px = fb[(my as u32 * sw + mx as u32) as usize];
                assert_eq!(px, COLOR_CURSOR, "cursor apex at ({mx},{my})");
            }
        }
    }
}
