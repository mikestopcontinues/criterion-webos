// SPDX-License-Identifier: GPL-3.0-or-later
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Surface {
    pub window: Size,
    pub drawable: Size,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}
impl Surface {
    pub fn fit(self, logical: Size) -> Option<Viewport> {
        if [
            self.window.width,
            self.window.height,
            self.drawable.width,
            self.drawable.height,
            logical.width,
            logical.height,
        ]
        .contains(&0)
        {
            return None;
        }
        let scale = (f64::from(self.drawable.width) / f64::from(logical.width))
            .min(f64::from(self.drawable.height) / f64::from(logical.height));
        let width = (f64::from(logical.width) * scale).round() as u32;
        let height = (f64::from(logical.height) * scale).round() as u32;
        if width == 0 || height == 0 {
            return None;
        }
        Some(Viewport {
            x: (self.drawable.width - width) / 2,
            y: (self.drawable.height - height) / 2,
            width,
            height,
            scale,
        })
    }
    pub fn pointer_to_logical(self, logical: Size, position: [f32; 2]) -> Option<[f32; 2]> {
        let fit = self.fit(logical)?;
        let x = (f64::from(position[0]) * f64::from(self.drawable.width)
            / f64::from(self.window.width)
            - f64::from(fit.x))
            / fit.scale;
        let y = (f64::from(position[1]) * f64::from(self.drawable.height)
            / f64::from(self.window.height)
            - f64::from(fit.y))
            / fit.scale;
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= f64::from(logical.width)
            || y >= f64::from(logical.height)
        {
            return None;
        }
        Some([x as f32, y as f32])
    }
}
