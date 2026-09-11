// 2D accelerator (DMA2D). Register layout and command numbers are in
// constants.rs; this file is the datapath. Every command reads the destination
// rectangle from DEST/WIDTH/HEIGHT/STRIDE and writes VRAM; the copy and mask
// commands additionally read pixels or coverage bytes from SRC/SRC_STRIDE, and
// the shape commands take RADIUS/SPREAD. A scissor rectangle (CLIP_*) limits
// every write, so the guest can repaint a damaged region without reshaping
// what it draws.
//
// Alpha is 8-bit straight (not premultiplied): out = dst + (src - dst) * a / 255.
// A source pixel with alpha 0 leaves the destination untouched, alpha 255 is
// an opaque copy. The guest's graphics.rgb() packs 0x00RRGGBB, so opaque
// commands ignore the top byte rather than treating it as "transparent", and
// the shape commands treat alpha 0 as 255 for the same reason.
use crate::constants::{
    is_ram_address, is_vram_address, DISPLAY_HEIGHT, DISPLAY_WIDTH, DMA2D_CLIP_X0_ADDR,
    DMA2D_CLIP_X1_ADDR, DMA2D_CLIP_Y0_ADDR, DMA2D_CLIP_Y1_ADDR, DMA2D_CMD_BLEND_FILL,
    DMA2D_CMD_COPY, DMA2D_CMD_COPY_BLEND, DMA2D_CMD_FILL, DMA2D_CMD_GRADIENT_H,
    DMA2D_CMD_GRADIENT_V, DMA2D_CMD_MASK_A8, DMA2D_CMD_ROUND_RECT, DMA2D_CMD_ROUND_RECT_OUTLINE,
    DMA2D_CMD_SHADOW, DMA2D_COLOR2_ADDR, DMA2D_COLOR_ADDR, DMA2D_DEST_ADDR, DMA2D_HEIGHT_ADDR,
    DMA2D_RADIUS_ADDR, DMA2D_SPREAD_ADDR, DMA2D_SRC_ADDR, DMA2D_SRC_STRIDE_ADDR,
    DMA2D_STRIDE_ADDR, DMA2D_WIDTH_ADDR, VRAM_BASE,
};

use super::Machine;

// Blend `src` over `dst` with 8-bit coverage/alpha `a`, per channel.
#[inline]
pub fn blend_pixel(dst: u32, src: u32, a: u32) -> u32 {
    if a == 0 {
        return dst;
    }
    if a >= 255 {
        return src & 0x00FF_FFFF;
    }
    let mut out = 0u32;
    for shift in [0u32, 8, 16] {
        let d = (dst >> shift) & 0xFF;
        let s = (src >> shift) & 0xFF;
        // +127 rounds to nearest instead of truncating toward the destination,
        // so a 50% blend of black over white is 128, not 127.
        let c = (d * (255 - a) + s * a + 127) / 255;
        out |= c << shift;
    }
    out
}

// Linear interpolation between two packed colours; t is 0..=255.
#[inline]
pub fn lerp_color(c0: u32, c1: u32, t: u32) -> u32 {
    blend_pixel(c0, c1, t)
}

// Coverage (0..=1) of the pixel centred at (px, py) by a rounded rectangle of
// size w x h at the origin with corner radius r: 1 inside, 0 outside, and a
// one-pixel anti-aliased ramp across the edge.
#[inline]
pub fn round_rect_coverage(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    // Signed distance to the rounded box (Inigo Quilez's sdRoundedBox).
    let hx = w * 0.5;
    let hy = h * 0.5;
    let r = r.min(hx).min(hy).max(0.0);
    let qx = (px - hx).abs() - hx + r;
    let qy = (py - hy).abs() - hy + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    let d = outside + inside - r;
    (0.5 - d).clamp(0.0, 1.0)
}

// Signed distance from (px, py) to the same rounded box, positive outside.
#[inline]
fn round_rect_distance(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let hx = w * 0.5;
    let hy = h * 0.5;
    let r = r.min(hx).min(hy).max(0.0);
    let qx = (px - hx).abs() - hx + r;
    let qy = (py - hy).abs() - hy + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    outside + inside - r
}

// The destination rectangle after the scissor: `x0..x1` / `y0..y1` are the
// pixels actually written, `ox`/`oy` their offset from the unclipped origin
// (so per-pixel shapes and source fetches stay aligned with the full rect).
struct Dest {
    origin: usize, // vram index of the unclipped top-left
    stride: usize,
    w: usize, // unclipped size
    h: usize,
    x0: usize, // clipped range, relative to the unclipped origin
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Machine {
    fn dma2d_reg(&self, addr: u32, default: u32) -> u32 {
        *self.io.get(&addr).unwrap_or(&default)
    }

    // Read the destination registers and intersect with the scissor. The
    // scissor is in display coordinates, so it is only meaningful when the
    // destination stride is the display width (the only stride the guest
    // uses); other strides skip it.
    fn dma2d_dest(&self) -> Option<Dest> {
        let dest = self.dma2d_reg(DMA2D_DEST_ADDR, 0);
        if dest < VRAM_BASE {
            return None;
        }
        let origin = (dest - VRAM_BASE) as usize / 4;
        let w = self.dma2d_reg(DMA2D_WIDTH_ADDR, 0) as usize;
        let h = self.dma2d_reg(DMA2D_HEIGHT_ADDR, 0) as usize;
        let stride = self.dma2d_reg(DMA2D_STRIDE_ADDR, DISPLAY_WIDTH as u32) as usize;
        if w == 0 || h == 0 || stride == 0 {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (0usize, 0usize, w, h);
        let cx0 = self.dma2d_reg(DMA2D_CLIP_X0_ADDR, 0) as i64;
        let cy0 = self.dma2d_reg(DMA2D_CLIP_Y0_ADDR, 0) as i64;
        let cx1 = self.dma2d_reg(DMA2D_CLIP_X1_ADDR, 0) as i64;
        let cy1 = self.dma2d_reg(DMA2D_CLIP_Y1_ADDR, 0) as i64;
        if stride == DISPLAY_WIDTH && cx1 > cx0 && cy1 > cy0 {
            let dx = (origin % stride) as i64;
            let dy = (origin / stride) as i64;
            x0 = (cx0 - dx).clamp(0, w as i64) as usize;
            y0 = (cy0 - dy).clamp(0, h as i64) as usize;
            x1 = (cx1 - dx).clamp(0, w as i64) as usize;
            y1 = (cy1 - dy).clamp(0, h as i64) as usize;
        }
        // Never run past the end of VRAM.
        let rows_left = (self.vram.len().saturating_sub(origin) + stride - 1) / stride;
        y1 = y1.min(rows_left);
        if stride == DISPLAY_WIDTH {
            let cols_left = DISPLAY_WIDTH - (origin % stride);
            x1 = x1.min(cols_left);
            let total_rows = DISPLAY_HEIGHT.saturating_sub(origin / stride);
            y1 = y1.min(total_rows);
        }
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Dest {
            origin,
            stride,
            w,
            h,
            x0,
            y0,
            x1,
            y1,
        })
    }

    // Read one source pixel (a 32-bit word) from RAM or VRAM. Sources in the
    // ROM/IO windows read as 0, like an unmapped DMA fetch would.
    fn dma2d_src_pixel(&self, addr: u32) -> u32 {
        if is_vram_address(addr) {
            let idx = (addr - VRAM_BASE) as usize / 4;
            return *self.vram.get(idx).unwrap_or(&0);
        }
        if is_ram_address(addr) {
            return self.ram_read_word(addr);
        }
        0
    }

    fn dma2d_src_byte(&self, addr: u32) -> u8 {
        if is_ram_address(addr) {
            return *self.ram.get(addr as usize).unwrap_or(&0);
        }
        0
    }

    // Run `f(x, y, dst) -> new` over every clipped destination pixel; x/y are
    // relative to the unclipped rectangle origin.
    fn dma2d_for_each<F: FnMut(usize, usize, u32) -> u32>(&mut self, d: &Dest, mut f: F) {
        for y in d.y0..d.y1 {
            let row = d.origin + y * d.stride;
            if row + d.x1 > self.vram.len() {
                break;
            }
            for x in d.x0..d.x1 {
                let px = &mut self.vram[row + x];
                *px = f(x, y, *px);
            }
        }
    }

    pub(super) fn service_dma2d(&mut self, cmd: u32) {
        let Some(d) = self.dma2d_dest() else {
            return;
        };
        let color = self.dma2d_reg(DMA2D_COLOR_ADDR, 0);
        match cmd {
            DMA2D_CMD_FILL => {
                let c = color & 0x00FF_FFFF;
                for y in d.y0..d.y1 {
                    let row = d.origin + y * d.stride;
                    if row + d.x1 <= self.vram.len() {
                        self.vram[row + d.x0..row + d.x1].fill(c);
                    }
                }
            }
            DMA2D_CMD_BLEND_FILL => {
                let a = color >> 24;
                self.dma2d_for_each(&d, |_, _, dst| blend_pixel(dst, color, a));
            }
            DMA2D_CMD_COPY | DMA2D_CMD_COPY_BLEND => {
                let src = self.dma2d_reg(DMA2D_SRC_ADDR, 0);
                let src_stride = self.dma2d_reg(DMA2D_SRC_STRIDE_ADDR, d.w as u32) as usize;
                // Stage the source first so a VRAM-to-VRAM copy of overlapping
                // rectangles (a window being dragged over itself) reads the
                // pre-copy pixels, the same as memmove.
                let cw = d.x1 - d.x0;
                let ch = d.y1 - d.y0;
                let mut staged = vec![0u32; cw * ch];
                for y in 0..ch {
                    for x in 0..cw {
                        let sy = y + d.y0;
                        let sx = x + d.x0;
                        let addr = src.wrapping_add(((sy * src_stride + sx) * 4) as u32);
                        staged[y * cw + x] = self.dma2d_src_pixel(addr);
                    }
                }
                let blend = cmd == DMA2D_CMD_COPY_BLEND;
                self.dma2d_for_each(&d, |x, y, dst| {
                    let s = staged[(y - d.y0) * cw + (x - d.x0)];
                    if blend {
                        blend_pixel(dst, s, s >> 24)
                    } else {
                        s & 0x00FF_FFFF
                    }
                });
            }
            DMA2D_CMD_MASK_A8 => {
                let src = self.dma2d_reg(DMA2D_SRC_ADDR, 0);
                let src_stride = self.dma2d_reg(DMA2D_SRC_STRIDE_ADDR, d.w as u32) as usize;
                let cw = d.x1 - d.x0;
                let ch = d.y1 - d.y0;
                let mut staged = vec![0u8; cw * ch];
                for y in 0..ch {
                    for x in 0..cw {
                        let addr = src.wrapping_add(((y + d.y0) * src_stride + x + d.x0) as u32);
                        staged[y * cw + x] = self.dma2d_src_byte(addr);
                    }
                }
                self.dma2d_for_each(&d, |x, y, dst| {
                    let a = staged[(y - d.y0) * cw + (x - d.x0)] as u32;
                    blend_pixel(dst, color, a)
                });
            }
            DMA2D_CMD_GRADIENT_V | DMA2D_CMD_GRADIENT_H => {
                let color2 = self.dma2d_reg(DMA2D_COLOR2_ADDR, color);
                if cmd == DMA2D_CMD_GRADIENT_V {
                    let span = d.h.max(2) - 1;
                    for y in d.y0..d.y1 {
                        let c = lerp_color(color, color2, (y * 255 / span) as u32);
                        let row = d.origin + y * d.stride;
                        if row + d.x1 <= self.vram.len() {
                            self.vram[row + d.x0..row + d.x1].fill(c);
                        }
                    }
                } else {
                    let span = d.w.max(2) - 1;
                    let line: Vec<u32> = (d.x0..d.x1)
                        .map(|x| lerp_color(color, color2, (x * 255 / span) as u32))
                        .collect();
                    for y in d.y0..d.y1 {
                        let row = d.origin + y * d.stride;
                        if row + d.x1 <= self.vram.len() {
                            self.vram[row + d.x0..row + d.x1].copy_from_slice(&line);
                        }
                    }
                }
            }
            DMA2D_CMD_ROUND_RECT | DMA2D_CMD_ROUND_RECT_OUTLINE => {
                let r = self.dma2d_reg(DMA2D_RADIUS_ADDR, 0) as f32;
                let t = self.dma2d_reg(DMA2D_SPREAD_ADDR, 1).max(1) as f32;
                let mut a = color >> 24;
                if a == 0 {
                    a = 255;
                }
                let (w, h) = (d.w as f32, d.h as f32);
                let outline = cmd == DMA2D_CMD_ROUND_RECT_OUTLINE;
                self.dma2d_for_each(&d, |x, y, dst| {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let mut cov = round_rect_coverage(px, py, w, h, r);
                    if outline {
                        // Ring: outer shape minus the same shape inset by t.
                        let inner = round_rect_coverage(px - t, py - t, w - 2.0 * t, h - 2.0 * t, (r - t).max(0.0));
                        cov = (cov - inner).max(0.0);
                    }
                    blend_pixel(dst, color, (cov * a as f32 + 0.5) as u32)
                });
            }
            DMA2D_CMD_SHADOW => {
                // DEST is the rectangle casting the shadow; the shadow itself
                // extends SPREAD pixels beyond it on every side, so the guest
                // must set the scissor (or accept writes there). Opacity is
                // COLOR.a at the edge, falling off quadratically to 0 at
                // SPREAD, which reads as a Gaussian-ish blur. Inside the
                // rectangle it is solid; the caster is expected to paint
                // over it.
                let r = self.dma2d_reg(DMA2D_RADIUS_ADDR, 0) as f32;
                let spread = self.dma2d_reg(DMA2D_SPREAD_ADDR, 8).max(1) as usize;
                let mut a_max = color >> 24;
                if a_max == 0 {
                    a_max = 128;
                }
                let (w, h) = (d.w as f32, d.h as f32);
                // Expand the destination by `spread` on each side (clamped to
                // the display) and evaluate the distance field relative to
                // the original rectangle.
                let ox = (d.origin % d.stride) as i64;
                let oy = (d.origin / d.stride) as i64;
                let ex0 = (ox - spread as i64).max(0);
                let ey0 = (oy - spread as i64).max(0);
                let ex1 = (ox + d.w as i64 + spread as i64).min(DISPLAY_WIDTH as i64);
                let ey1 = (oy + d.h as i64 + spread as i64).min(DISPLAY_HEIGHT as i64);
                // Scissor in display coordinates, if enabled.
                let cx0 = self.dma2d_reg(DMA2D_CLIP_X0_ADDR, 0) as i64;
                let cy0 = self.dma2d_reg(DMA2D_CLIP_Y0_ADDR, 0) as i64;
                let cx1 = self.dma2d_reg(DMA2D_CLIP_X1_ADDR, 0) as i64;
                let cy1 = self.dma2d_reg(DMA2D_CLIP_Y1_ADDR, 0) as i64;
                let (ex0, ey0, ex1, ey1) = if cx1 > cx0 && cy1 > cy0 {
                    (ex0.max(cx0), ey0.max(cy0), ex1.min(cx1), ey1.min(cy1))
                } else {
                    (ex0, ey0, ex1, ey1)
                };
                if d.stride != DISPLAY_WIDTH || ex1 <= ex0 || ey1 <= ey0 {
                    return;
                }
                let spread_f = spread as f32;
                for sy in ey0..ey1 {
                    let row = sy as usize * d.stride;
                    for sx in ex0..ex1 {
                        let px = (sx - ox) as f32 + 0.5;
                        let py = (sy - oy) as f32 + 0.5;
                        let dist = round_rect_distance(px, py, w, h, r);
                        let t = (1.0 - dist / spread_f).clamp(0.0, 1.0);
                        let a = (a_max as f32 * t * t + 0.5) as u32;
                        if a > 0 {
                            let idx = row + sx as usize;
                            if idx < self.vram.len() {
                                self.vram[idx] = blend_pixel(self.vram[idx], color, a);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{blend_pixel, lerp_color, round_rect_coverage};

    #[test]
    fn alpha_extremes_are_identity_and_copy() {
        assert_eq!(blend_pixel(0x123456, 0xABCDEF, 0), 0x123456);
        assert_eq!(blend_pixel(0x123456, 0xFFABCDEF, 255), 0xABCDEF);
    }

    #[test]
    fn half_alpha_lands_in_the_middle() {
        assert_eq!(blend_pixel(0x000000, 0xFFFFFF, 128), 0x808080);
        assert_eq!(blend_pixel(0xFFFFFF, 0x000000, 128), 0x7F7F7F);
    }

    #[test]
    fn lerp_endpoints() {
        assert_eq!(lerp_color(0x102030, 0x405060, 0), 0x102030);
        assert_eq!(lerp_color(0x102030, 0x405060, 255), 0x405060);
    }

    #[test]
    fn round_rect_coverage_inside_outside_and_corner() {
        // Centre of a 20x10 box with r=4 is fully covered.
        assert_eq!(round_rect_coverage(10.0, 5.0, 20.0, 10.0, 4.0), 1.0);
        // Far outside is empty; the corner pixel is cut away by the radius.
        assert_eq!(round_rect_coverage(30.0, 5.0, 20.0, 10.0, 4.0), 0.0);
        assert!(round_rect_coverage(0.5, 0.5, 20.0, 10.0, 4.0) < 0.2);
        // With r=0 the corner pixel is fully inside.
        assert_eq!(round_rect_coverage(0.5, 0.5, 20.0, 10.0, 0.0), 1.0);
    }
}
