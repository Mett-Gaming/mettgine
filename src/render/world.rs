//! Daten, die sich selten aendern, in GPU-Puffer laden: Welt-Boxen mit Lampen und feste Modelle.

use super::*;
use crate::gpu::{GpuInstance, GpuLight};

impl Renderer {
    /// Laedt die Boxen und Lampen der Welt hoch (wartet auf die GPU, nur bei Weltwechsel).
    pub fn upload_world(&mut self, boxes: &[GpuInstance], lights: &[GpuLight]) -> Result<()> {
        self.wait_idle()?;
        fill(&self.ctx, &mut self.box_buf, bytemuck::cast_slice(boxes))?;
        fill(&self.ctx, &mut self.light_buf, bytemuck::cast_slice(lights))?;
        self.boxes = boxes.to_vec();
        self.box_count = boxes.len() as u32;
        self.light_count = lights.len() as u32;
        self.write_all_descriptors();
        Ok(())
    }

    /// Feste Modelle (wartet auf die GPU, nur bei Aenderungen). Werden nach Modell sortiert und
    /// je Modell mit einem einzigen Zeichenbefehl gezeichnet; schwanken mit den Boxen mit.
    pub fn upload_statics(&mut self, statics: &[MeshDraw]) -> Result<()> {
        let known = self.mesh_ranges.len();
        let mut sorted: Vec<&MeshDraw> = statics.iter().filter(|m| m.mesh < known).collect();
        sorted.sort_by_key(|m| m.mesh);
        let insts: Vec<GpuInst> = sorted.iter().map(|m| GpuInst::mesh(m, true)).collect();
        let mut groups: Vec<Group> = Vec::new();
        for (i, m) in sorted.iter().enumerate() {
            match groups.last_mut() {
                Some(g) if g.mesh == m.mesh as u32 => g.count += 1,
                _ => groups.push(Group {
                    mesh: m.mesh as u32,
                    first_inst: i as u32,
                    count: 1,
                }),
            }
        }
        self.wait_idle()?;
        fill(
            &self.ctx,
            &mut self.static_buf,
            bytemuck::cast_slice(&insts),
        )?;
        self.statics = statics.to_vec();
        self.static_groups = groups;
        self.write_all_descriptors();
        Ok(())
    }

    fn wait_idle(&self) -> Result<()> {
        // SAFETY: Das Geraet lebt so lange wie der Renderer.
        unsafe { self.ctx.device.device_wait_idle()? };
        Ok(())
    }

    fn write_all_descriptors(&self) {
        for fi in 0..FRAMES_IN_FLIGHT {
            self.write_scene_descriptors(fi);
        }
    }
}

fn fill(ctx: &Context, buf: &mut Buffer, bytes: &[u8]) -> Result<()> {
    let usage = vk::BufferUsageFlags::STORAGE_BUFFER;
    buf.ensure(ctx, bytes.len() as u64, usage)?;
    buf.write(bytes);
    Ok(())
}
