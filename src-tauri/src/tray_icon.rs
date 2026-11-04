//! Renders the tray icon (two usage bars, or a "!" error badge) into RGBA pixels.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarColor {
    Blue,
    Orange,
    Red,
}

pub const BLUE: [u8; 3] = [0x4A, 0x90, 0xE2];
pub const ORANGE: [u8; 3] = [0xF5, 0xA6, 0x23];
pub const RED: [u8; 3] = [0xE5, 0x48, 0x4D];

impl BarColor {
    pub fn rgb(self) -> [u8; 3] {
        match self {
            BarColor::Blue => BLUE,
            BarColor::Orange => ORANGE,
            BarColor::Red => RED,
        }
    }

    pub fn css(self) -> &'static str {
        match self {
            BarColor::Blue => "blue",
            BarColor::Orange => "orange",
            BarColor::Red => "red",
        }
    }
}

/// ≥ 95 % or ahead of pace → red; ≥ 80 % → orange; otherwise blue.
pub fn bar_color(utilization: f64, ahead: bool) -> BarColor {
    if utilization >= 95.0 || ahead {
        BarColor::Red
    } else if utilization >= 80.0 {
        BarColor::Orange
    } else {
        BarColor::Blue
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub utilization: f64,
    pub color: BarColor,
}

/// `(top y, height)` of the two bar rows for an icon of `size` pixels.
pub fn bar_rows(size: u32) -> [(u32, u32); 2] {
    let h = ((size as f32) * 0.35).round().max(1.0) as u32;
    let gap = ((size as f32) * 0.1).round().max(1.0) as u32;
    let top = size.saturating_sub(2 * h + gap) / 2;
    [(top, h), (top + h + gap, h)]
}

struct Canvas {
    size: u32,
    px: Vec<u8>,
}

impl Canvas {
    fn put(&mut self, x: u32, y: u32, rgb: [u8; 3], a: u8) {
        if x < self.size && y < self.size {
            let i = ((y * self.size + x) * 4) as usize;
            self.px[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
        }
    }

    fn rect(&mut self, x0: u32, y0: u32, w: u32, h: u32, rgb: [u8; 3], a: u8) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                self.put(x, y, rgb, a);
            }
        }
    }
}

/// RGBA buffer of `size × size` pixels.
pub fn render(bars: &[Option<Bar>], error: bool, size: u32, dark_panel: bool) -> Vec<u8> {
    let mut c = Canvas {
        size,
        px: vec![0; (size * size * 4) as usize],
    };
    if error {
        draw_error(&mut c);
        return c.px;
    }
    let track = if dark_panel {
        [255, 255, 255]
    } else {
        [0, 0, 0]
    };
    let width = size.saturating_sub(2);
    for (i, (y0, h)) in bar_rows(size).into_iter().enumerate() {
        c.rect(1, y0, width, h, track, 80);
        if let Some(Some(bar)) = bars.get(i) {
            let frac = bar.utilization.clamp(0.0, 100.0) / 100.0;
            let fill = (width as f64 * frac).round() as u32;
            c.rect(1, y0, fill, h, bar.color.rgb(), 255);
        }
    }
    c.px
}

fn draw_error(c: &mut Canvas) {
    let s = c.size as f32;
    let (cx, r) = (s / 2.0 - 0.5, s / 2.0 - 0.5);
    for y in 0..c.size {
        for x in 0..c.size {
            let (dx, dy) = (x as f32 - cx, y as f32 - cx);
            if dx * dx + dy * dy <= r * r {
                c.put(x, y, RED, 255);
            }
        }
    }
    let w = (s * 0.14).round().max(1.0) as u32;
    let x0 = (c.size - w) / 2;
    let top = (s * 0.18).round() as u32;
    let bar_h = (s * 0.42).round() as u32;
    c.rect(x0, top, w, bar_h, [255, 255, 255], 255);
    let dot = (s * 0.70).round() as u32;
    c.rect(x0, dot, w, w, [255, 255, 255], 255);
}

/// Whether the taskbar/panel is dark. Windows reads the system theme;
/// other platforms assume a dark panel.
pub fn panel_is_dark() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                "/v",
                "SystemUsesLightTheme",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        if let Ok(out) = out {
            return !String::from_utf8_lossy(&out.stdout).contains("0x1");
        }
        true
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = 32;

    fn rgb_at(px: &[u8], size: u32, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * size + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    }

    fn top_bar_y() -> u32 {
        let (y0, h) = bar_rows(SIZE)[0];
        y0 + h / 2
    }

    #[test]
    fn buffer_size() {
        assert_eq!(
            render(&[None, None], false, SIZE, true).len(),
            (SIZE * SIZE * 4) as usize
        );
    }

    #[test]
    fn colors() {
        assert_eq!(bar_color(50.0, false), BarColor::Blue);
        assert_eq!(bar_color(80.0, false), BarColor::Orange);
        assert_eq!(bar_color(95.0, false), BarColor::Red);
        assert_eq!(bar_color(10.0, true), BarColor::Red);
    }

    #[test]
    fn fill_proportional() {
        let px = render(
            &[
                Some(Bar {
                    utilization: 50.0,
                    color: BarColor::Blue,
                }),
                None,
            ],
            false,
            SIZE,
            true,
        );
        assert_eq!(rgb_at(&px, SIZE, 8, top_bar_y()), BLUE);
        assert_ne!(rgb_at(&px, SIZE, 24, top_bar_y()), BLUE);
    }

    #[test]
    fn over_100_clamped() {
        let px = render(
            &[
                Some(Bar {
                    utilization: 150.0,
                    color: BarColor::Blue,
                }),
                None,
            ],
            false,
            SIZE,
            true,
        );
        assert_eq!(rgb_at(&px, SIZE, SIZE - 3, top_bar_y()), BLUE);
        let neg = render(
            &[
                Some(Bar {
                    utilization: -5.0,
                    color: BarColor::Blue,
                }),
                None,
            ],
            false,
            SIZE,
            true,
        );
        assert_ne!(rgb_at(&neg, SIZE, 2, top_bar_y()), BLUE);
    }

    #[test]
    fn second_bar_uses_its_own_row() {
        let px = render(
            &[
                None,
                Some(Bar {
                    utilization: 100.0,
                    color: BarColor::Red,
                }),
            ],
            false,
            SIZE,
            true,
        );
        let (y0, h) = bar_rows(SIZE)[1];
        assert_eq!(rgb_at(&px, SIZE, 16, y0 + h / 2), RED);
        assert_ne!(rgb_at(&px, SIZE, 16, top_bar_y()), RED);
    }

    #[test]
    fn light_and_dark_panels_differ() {
        assert_ne!(
            render(&[None, None], false, SIZE, true),
            render(&[None, None], false, SIZE, false)
        );
    }

    #[test]
    fn error_icon_differs() {
        assert_ne!(
            render(&[None, None], true, SIZE, true),
            render(&[None, None], false, SIZE, true)
        );
    }

    #[test]
    fn other_sizes_render() {
        for size in [16, 22, 44, 64] {
            assert_eq!(
                render(&[None, None], true, size, false).len(),
                (size * size * 4) as usize
            );
        }
    }
}
