//! Daten, die sich selten aendern, in GPU-Puffer laden: Welt-Boxen und Lichter, Waren in
//! den Regalen; danach die Descriptor-Sets neu schreiben.

use super::*;

impl Renderer {
    /// Laedt die Boxen und Lichter der Welt hoch (wartet auf die GPU, nur bei Weltwechsel).
    pub fn upload_world(&mut self, instances: &[GpuInstance], lights: &[GpuLight]) -> Result<()> {
        let inst_bytes: &[u8] = bytemuck::cast_slice(instances);
        let light_bytes: &[u8] = bytemuck::cast_slice(lights);
        unsafe { self.ctx.device.device_wait_idle()? };

        let usage = vk::BufferUsageFlags::STORAGE_BUFFER;
        if inst_bytes.len() as u64 > self.instance_buf.size {
            let new = Buffer::new_host(
                &self.ctx,
                (inst_bytes.len() as u64).next_power_of_two(),
                usage,
            )?;
            std::mem::replace(&mut self.instance_buf, new).destroy(&self.ctx.device);
        }
        if light_bytes.len() as u64 > self.light_buf.size {
            let new = Buffer::new_host(
                &self.ctx,
                (light_bytes.len() as u64).next_power_of_two(),
                usage,
            )?;
            std::mem::replace(&mut self.light_buf, new).destroy(&self.ctx.device);
        }
        self.instance_buf.write(inst_bytes);
        self.light_buf.write(light_bytes);
        self.instance_count = instances.len() as u32;
        self.light_count = lights.len() as u32;
        self.write_scene_descriptors();
        Ok(())
    }

    /// Statische Mesh-Instanzen (Waren). Werden nach Modell sortiert und pro Modell mit
    /// einem einzigen instanzierten Draw-Call gezeichnet.
    pub fn upload_statics(&mut self, statics: &[MeshDraw]) -> Result<()> {
        let mut sorted: Vec<&MeshDraw> = statics.iter().collect();
        sorted.sort_by_key(|m| m.mesh);
        let insts: Vec<GpuMeshInst> = sorted
            .iter()
            .map(|m| GpuMeshInst {
                model: m.model.to_cols_array_2d(),
                tint: [m.tint.x, m.tint.y, m.tint.z, 1.0],
                extra: [m.tint.w, 0.0, 1.0, 0.0],
            })
            .collect();
        let mut ranges: Vec<(u32, u32, u32)> = Vec::new();
        for (i, m) in sorted.iter().enumerate() {
            match ranges.last_mut() {
                Some(r) if r.0 == m.mesh as u32 => r.2 += 1,
                _ => ranges.push((m.mesh as u32, i as u32, 1)),
            }
        }
        let bytes: &[u8] = bytemuck::cast_slice(&insts);
        unsafe { self.ctx.device.device_wait_idle()? };
        if bytes.len() as u64 > self.static_buf.size {
            let new = Buffer::new_host(
                &self.ctx,
                (bytes.len() as u64).next_power_of_two(),
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;
            std::mem::replace(&mut self.static_buf, new).destroy(&self.ctx.device);
        }
        self.static_buf.write(bytes);
        self.static_ranges = ranges;
        self.write_scene_descriptors();
        Ok(())
    }

    pub(super) fn write_scene_descriptors(&mut self) {
        let light = [vk::DescriptorBufferInfo::default()
            .buffer(self.light_buf.buffer)
            .range(vk::WHOLE_SIZE)];
        // (Set, Instanzen, Knochen, Monitor-Bild an binding 5)
        let mut targets = vec![
            (
                self.world_set,
                self.instance_buf.buffer,
                self.bone_bufs[0].buffer,
                true,
            ),
            (
                self.world_set_cam,
                self.instance_buf.buffer,
                self.bone_bufs[0].buffer,
                false,
            ),
            (
                self.static_set,
                self.static_buf.buffer,
                self.bone_bufs[0].buffer,
                true,
            ),
        ];
        for (i, &s) in self.mesh_sets.iter().enumerate() {
            targets.push((
                s,
                self.mesh_inst_bufs[i].buffer,
                self.bone_bufs[i].buffer,
                true,
            ));
        }
        let sign_info = [vk::DescriptorImageInfo::default()
            .image_view(self.signs.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let sign_sampler = [vk::DescriptorImageInfo::default().sampler(self.sign_sampler)];
        let monitor_info = [vk::DescriptorImageInfo::default()
            .image_view(self.monitor.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        // Platzhalter fuer den Kamera-Pass: der Schild-Atlas (wird dort nie abgetastet)
        let no_monitor = [vk::DescriptorImageInfo::default()
            .image_view(self.signs.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        for (set, buf, bones, with_monitor) in targets {
            let inst = [vk::DescriptorBufferInfo::default()
                .buffer(buf)
                .range(vk::WHOLE_SIZE)];
            let bone_info = [vk::DescriptorBufferInfo::default()
                .buffer(bones)
                .range(vk::WHOLE_SIZE)];
            unsafe {
                self.ctx.device.update_descriptor_sets(
                    &[
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(0)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .buffer_info(&inst),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(1)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .buffer_info(&light),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(2)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .image_info(&sign_info),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(3)
                            .descriptor_type(vk::DescriptorType::SAMPLER)
                            .image_info(&sign_sampler),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(4)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .buffer_info(&bone_info),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(5)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .image_info(if with_monitor {
                                &monitor_info
                            } else {
                                &no_monitor
                            }),
                    ],
                    &[],
                );
            }
        }
    }
}
