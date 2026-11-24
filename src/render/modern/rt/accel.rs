//! Beschleunigungsstrukturen anlegen und bauen.

use super::super::super::context::Context;
use super::super::super::device::RtFns;
use super::super::super::Result;
use super::super::memory::{Arena, GpuBuffer};
use super::super::model::Vertex;
use ash::vk;

/// Verwendung von Arbeitsspeicher beim Bauen
pub const SCRATCH: vk::BufferUsageFlags = vk::BufferUsageFlags::from_raw(
    vk::BufferUsageFlags::STORAGE_BUFFER.as_raw()
        | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS.as_raw(),
);
const BOTTOM: vk::AccelerationStructureTypeKHR = vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL;
const TOP: vk::AccelerationStructureTypeKHR = vk::AccelerationStructureTypeKHR::TOP_LEVEL;
/// alles zaehlt als undurchsichtig
const OPAQUE: vk::GeometryFlagsKHR = vk::GeometryFlagsKHR::OPAQUE;

/// Eine Beschleunigungsstruktur.
pub struct Accel {
    pub handle: vk::AccelerationStructureKHR,
    pub address: u64,
    buffer: GpuBuffer,
    /// groesste Eckpunkt-Nummer und Dreiecke (Modelle)
    max_vertex: u32,
    primitives: u32,
    scratch: u64,
    /// wird jedes Bild neu gebaut
    fast: bool,
}

fn rt(ctx: &Context) -> Result<&RtFns> {
    ctx.rt
        .as_ref()
        .ok_or_else(|| "Raytracing ist nicht eingeschaltet".into())
}

unsafe fn address(ctx: &Context, buffer: vk::Buffer) -> u64 {
    let info = vk::BufferDeviceAddressInfo::default().buffer(buffer);
    ctx.device.get_buffer_device_address(&info)
}

fn triangles(
    vertices: u64,
    indices: u64,
    max_vertex: u32,
) -> vk::AccelerationStructureGeometryKHR<'static> {
    let data = vk::AccelerationStructureGeometryTrianglesDataKHR::default()
        .vertex_format(vk::Format::R32G32B32_SFLOAT)
        .vertex_data(vk::DeviceOrHostAddressConstKHR {
            device_address: vertices,
        })
        .vertex_stride(size_of::<Vertex>() as u64)
        .max_vertex(max_vertex)
        .index_type(vk::IndexType::UINT32)
        .index_data(vk::DeviceOrHostAddressConstKHR {
            device_address: indices,
        });
    vk::AccelerationStructureGeometryKHR::default()
        .geometry_type(vk::GeometryTypeKHR::TRIANGLES)
        .geometry(vk::AccelerationStructureGeometryDataKHR { triangles: data })
        .flags(OPAQUE)
}

/// Dreiecke ohne Index, je Eckpunkt vier Zahlen.
fn soup(vertices: u64, count: u32) -> vk::AccelerationStructureGeometryKHR<'static> {
    let data = vk::AccelerationStructureGeometryTrianglesDataKHR::default()
        .vertex_format(vk::Format::R32G32B32_SFLOAT)
        .vertex_data(vk::DeviceOrHostAddressConstKHR {
            device_address: vertices,
        })
        .vertex_stride(16)
        .max_vertex((count * 3).saturating_sub(1))
        .index_type(vk::IndexType::NONE_KHR);
    vk::AccelerationStructureGeometryKHR::default()
        .geometry_type(vk::GeometryTypeKHR::TRIANGLES)
        .geometry(vk::AccelerationStructureGeometryDataKHR { triangles: data })
        .flags(OPAQUE)
}

fn instances(data: u64) -> vk::AccelerationStructureGeometryKHR<'static> {
    let i = vk::AccelerationStructureGeometryInstancesDataKHR::default().data(
        vk::DeviceOrHostAddressConstKHR {
            device_address: data,
        },
    );
    vk::AccelerationStructureGeometryKHR::default()
        .geometry_type(vk::GeometryTypeKHR::INSTANCES)
        .geometry(vk::AccelerationStructureGeometryDataKHR { instances: i })
        .flags(OPAQUE)
}

/// `fast` = wird jedes Bild neu gebaut.
fn flags(fast: bool) -> vk::BuildAccelerationStructureFlagsKHR {
    if fast {
        vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_BUILD
    } else {
        vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE
    }
}

impl Accel {
    fn create(
        ctx: &Context,
        arena: &mut Arena,
        ty: vk::AccelerationStructureTypeKHR,
        geometry: vk::AccelerationStructureGeometryKHR,
        (max_vertex, primitives): (u32, u32),
        fast: bool,
    ) -> Result<Self> {
        let fns = rt(ctx)?;
        let geometries = [geometry];
        let info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .ty(ty)
            .flags(flags(fast))
            .geometries(&geometries);
        // SAFETY: gueltiges Geraet; freigegeben in `destroy`.
        unsafe {
            let mut sizes = vk::AccelerationStructureBuildSizesInfoKHR::default();
            fns.accel.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &info,
                &[primitives],
                &mut sizes,
            );
            let size = sizes.acceleration_structure_size;
            let usage = vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;
            let buffer = arena.buffer(ctx, size, usage)?;
            let handle = fns.accel.create_acceleration_structure(
                &vk::AccelerationStructureCreateInfoKHR::default()
                    .buffer(buffer.buffer)
                    .size(size)
                    .ty(ty),
                None,
            )?;
            let address = fns.accel.get_acceleration_structure_device_address(
                &vk::AccelerationStructureDeviceAddressInfoKHR::default()
                    .acceleration_structure(handle),
            );
            Ok(Self {
                handle,
                address,
                buffer,
                max_vertex,
                primitives,
                scratch: sizes.build_scratch_size,
                fast,
            })
        }
    }

    /// Fuer ein Modell.
    pub fn for_triangles(
        ctx: &Context,
        arena: &mut Arena,
        vertices: u32,
        count: u32,
    ) -> Result<Self> {
        let max_vertex = vertices.saturating_sub(1);
        let g = triangles(0, 0, max_vertex);
        Self::create(ctx, arena, BOTTOM, g, (max_vertex, count), false)
    }

    /// Fuer eine Figur mit bis zu `capacity` Dreiecken.
    pub fn for_figure(ctx: &Context, arena: &mut Arena, capacity: u32) -> Result<Self> {
        Self::create(ctx, arena, BOTTOM, soup(0, capacity), (0, capacity), true)
    }

    /// Fuer eine Szene mit bis zu `capacity` Objekten.
    pub fn for_instances(ctx: &Context, arena: &mut Arena, capacity: u32) -> Result<Self> {
        Self::create(ctx, arena, TOP, instances(0), (0, capacity), true)
    }

    /// Noetiger Arbeitsspeicher samt Spielraum zum Ausrichten.
    pub fn scratch_size(&self, ctx: &Context) -> u64 {
        self.scratch + ctx.rt.as_ref().map_or(0, |r| r.scratch_align)
    }

    unsafe fn build(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        ty: vk::AccelerationStructureTypeKHR,
        geometry: vk::AccelerationStructureGeometryKHR,
        scratch: vk::Buffer,
        count: u32,
    ) {
        let Some(fns) = &ctx.rt else {
            return;
        };
        let geometries = [geometry];
        let at = address(ctx, scratch).next_multiple_of(fns.scratch_align.max(1));
        let info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .ty(ty)
            .flags(flags(self.fast))
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .dst_acceleration_structure(self.handle)
            .geometries(&geometries)
            .scratch_data(vk::DeviceOrHostAddressKHR { device_address: at });
        let range = [vk::AccelerationStructureBuildRangeInfoKHR::default().primitive_count(count)];
        fns.accel
            .cmd_build_acceleration_structures(cmd, &[info], &[&range]);
    }

    /// Modell bauen; liefert den Arbeitsspeicher, der bis zum Ende des Bildes leben muss.
    ///
    /// # Safety
    /// `cmd` muss aufgenommen werden, die Puffer muessen gefuellt sein.
    pub unsafe fn build_triangles(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        arena: &mut Arena,
        vertices: vk::Buffer,
        indices: vk::Buffer,
    ) -> Result<GpuBuffer> {
        let scratch = arena.buffer(ctx, self.scratch_size(ctx), SCRATCH)?;
        let g = triangles(
            address(ctx, vertices),
            address(ctx, indices),
            self.max_vertex,
        );
        self.build(ctx, cmd, BOTTOM, g, scratch.buffer, self.primitives);
        Ok(scratch)
    }

    /// Figur bauen; `positions` = Puffer und Stelle darin.
    ///
    /// # Safety
    /// Wie [`Accel::build_triangles`]; `scratch` ist gross genug.
    pub unsafe fn build_figure(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        (positions, at): (vk::Buffer, u64),
        scratch: vk::Buffer,
        count: u32,
    ) {
        let g = soup(address(ctx, positions) + at, count);
        self.build(ctx, cmd, BOTTOM, g, scratch, count);
    }

    /// Szene bauen.
    ///
    /// # Safety
    /// Wie [`Accel::build_triangles`]; `scratch` ist gross genug.
    pub unsafe fn build_instances(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        scratch: vk::Buffer,
        data: vk::Buffer,
        count: u32,
    ) {
        let g = instances(address(ctx, data));
        self.build(ctx, cmd, TOP, g, scratch, count);
    }

    /// # Safety
    /// Die GPU darf die Struktur nicht mehr benutzen.
    pub unsafe fn destroy(&self, ctx: &Context, arena: &mut Arena) {
        if let Some(fns) = &ctx.rt {
            fns.accel.destroy_acceleration_structure(self.handle, None);
        }
        arena.drop_buffer(&ctx.device, &self.buffer);
    }
}

/// Ein Objekt der Szene: Lage zeilenweise (3 x 4), Nummer, Modell.
pub fn instance(rows: [f32; 12], index: u32, model: u64) -> vk::AccelerationStructureInstanceKHR {
    vk::AccelerationStructureInstanceKHR {
        transform: vk::TransformMatrixKHR { matrix: rows },
        instance_custom_index_and_mask: vk::Packed24_8::new(index & 0xff_ffff, 0xff),
        instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(
            0,
            vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8,
        ),
        acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
            device_handle: model,
        },
    }
}

/// Objekte als Bytes fuer den Puffer.
pub fn bytes(list: &[vk::AccelerationStructureInstanceKHR]) -> &[u8] {
    // SAFETY: einfache Daten ohne Luecken (64 Byte je Eintrag).
    unsafe { std::slice::from_raw_parts(list.as_ptr().cast(), std::mem::size_of_val(list)) }
}

unsafe fn memory(
    ctx: &Context,
    cmd: vk::CommandBuffer,
    (from, to): (vk::PipelineStageFlags, vk::PipelineStageFlags),
    (write, read): (vk::AccessFlags, vk::AccessFlags),
) {
    let b = vk::MemoryBarrier::default()
        .src_access_mask(write)
        .dst_access_mask(read);
    let none = vk::DependencyFlags::empty();
    ctx.device
        .cmd_pipeline_barrier(cmd, from, to, none, &[b], &[], &[]);
}

/// Kopierte Modelle duerfen gelesen werden.
///
/// # Safety
/// `cmd` muss aufgenommen werden.
pub unsafe fn after_copy(ctx: &Context, cmd: vk::CommandBuffer) {
    let build = vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_KHR;
    memory(
        ctx,
        cmd,
        (vk::PipelineStageFlags::TRANSFER, build),
        (
            vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::SHADER_READ,
        ),
    );
}

/// Vor dem Ausrechnen von Figuren: Kopiertes lesen, Ausgabe des vorigen Bildes ist egal.
///
/// # Safety
/// `cmd` muss aufgenommen werden.
pub unsafe fn before_figures(ctx: &Context, cmd: vk::CommandBuffer) {
    let compute = vk::PipelineStageFlags::COMPUTE_SHADER;
    memory(
        ctx,
        cmd,
        (
            vk::PipelineStageFlags::TRANSFER | vk::PipelineStageFlags::FRAGMENT_SHADER,
            compute,
        ),
        (
            vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE,
        ),
    );
}

/// Ausgerechnete Figuren duerfen gebaut und gelesen werden.
///
/// # Safety
/// `cmd` muss aufgenommen werden.
pub unsafe fn after_figures(ctx: &Context, cmd: vk::CommandBuffer) {
    let build = vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_KHR;
    memory(
        ctx,
        cmd,
        (
            vk::PipelineStageFlags::COMPUTE_SHADER,
            build | vk::PipelineStageFlags::FRAGMENT_SHADER,
        ),
        (vk::AccessFlags::SHADER_WRITE, vk::AccessFlags::SHADER_READ),
    );
}

/// Gebautes darf benutzt werden (von weiteren Bauten und von Strahlen).
///
/// # Safety
/// `cmd` muss aufgenommen werden.
pub unsafe fn after_build(ctx: &Context, cmd: vk::CommandBuffer) {
    let build = vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_KHR;
    memory(
        ctx,
        cmd,
        (build, build | vk::PipelineStageFlags::FRAGMENT_SHADER),
        (
            vk::AccessFlags::ACCELERATION_STRUCTURE_WRITE_KHR,
            vk::AccessFlags::ACCELERATION_STRUCTURE_READ_KHR
                | vk::AccessFlags::ACCELERATION_STRUCTURE_WRITE_KHR,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_entries_have_the_size_vulkan_expects() {
        let i = instance([0.0; 12], 7, 99);
        assert_eq!(bytes(&[i, i]).len(), 128);
        assert_eq!(i.instance_custom_index_and_mask.low_24(), 7);
        assert_eq!(i.instance_custom_index_and_mask.high_8(), 0xff);
    }
}
