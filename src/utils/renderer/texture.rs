pub struct TextureData<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

impl<'a> TextureData<'a> {
    #[inline(always)]
    pub fn sample_bilinear(&self, u: f32, v: f32) -> [u8; 3] {
        let u = u.rem_euclid(1.0);
        let v = v.rem_euclid(1.0);

        let fx = u * (self.width - 1) as f32;
        let fy = v * (self.height - 1) as f32;

        let x0 = fx.floor() as usize;
        let y0 = fy.floor() as usize;
        let x1 = (x0 + 1).min(self.width as usize - 1);
        let y1 = (y0 + 1).min(self.height as usize - 1);

        let wx = fx - x0 as f32;
        let wy = fy - y0 as f32;

        let idx00 = (y0 * self.width as usize + x0) * 4;
        let idx10 = (y0 * self.width as usize + x1) * 4;
        let idx01 = (y1 * self.width as usize + x0) * 4;
        let idx11 = (y1 * self.width as usize + x1) * 4;

        if idx11 + 2 < self.rgba.len() {
            let c00 = &self.rgba[idx00..idx00 + 3];
            let c10 = &self.rgba[idx10..idx10 + 3];
            let c01 = &self.rgba[idx01..idx01 + 3];
            let c11 = &self.rgba[idx11..idx11 + 3];

            let r = (1.0 - wx) * (1.0 - wy) * c00[0] as f32
                + wx * (1.0 - wy) * c10[0] as f32
                + (1.0 - wx) * wy * c01[0] as f32
                + wx * wy * c11[0] as f32;
            let g = (1.0 - wx) * (1.0 - wy) * c00[1] as f32
                + wx * (1.0 - wy) * c10[1] as f32
                + (1.0 - wx) * wy * c01[1] as f32
                + wx * wy * c11[1] as f32;
            let b = (1.0 - wx) * (1.0 - wy) * c00[2] as f32
                + wx * (1.0 - wy) * c10[2] as f32
                + (1.0 - wx) * wy * c01[2] as f32
                + wx * wy * c11[2] as f32;

            [
                r.clamp(0.0, 255.0) as u8,
                g.clamp(0.0, 255.0) as u8,
                b.clamp(0.0, 255.0) as u8,
            ]
        } else {
            [165, 175, 190]
        }
    }
}
