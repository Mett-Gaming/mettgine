//! Modelle, Texturen und Materialien im Grafikspeicher. Nachladen geht jederzeit, ohne auf die
//! GPU zu warten: Die Daten warten in Zwischenpuffern und werden am Anfang des naechsten Bildes
//! kopiert; Entferntes bleibt liegen, bis kein Bild im Flug es mehr benutzt.

use super::super::context::Context;
use super::super::util::Buffer;
use super::super::{Result, FRAMES_IN_FLIGHT};
use super::copy::copy_image;
use super::memory::{Arena, GpuBuffer, GpuImage, TexDesc};
use super::model::*;
use super::sets::MaterialSets;
use crate::cull::Sphere;
use ash::vk;
use ash::vk::Handle;
use bytemuck::{Pod, Zeroable};

/// Ein Modell im Grafikspeicher.
pub struct Model {
    pub vertices: GpuBuffer,
    pub indices: GpuBuffer,
    pub joints: Option<GpuBuffer>,
    pub index_count: u32,
    pub bounds: Sphere,
    /// schon im Grafikspeicher
    ready: bool,
}

/// Werte eines Materials, gleich aufgebaut wie `Mat` in `_m_common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GpuMaterial {
    base: [f32; 4],
    /// rgb = Eigenleuchten, w = Staerke der Normalenkarte (0 = keine)
    emissive: [f32; 4],
    /// Metall, Rauheit, Grenze der Loecher (0 = keine), Verdeckung an
    params: [f32; 4],
}

impl GpuMaterial {
    pub fn set_roughness(&mut self, r: f32) {
        self.params[1] = r;
    }
}

pub struct MaterialSlot {
    pub data: Material,
    pub set: vk::DescriptorSet,
}

/// Was nach einigen Bildern freigegeben wird.
enum Trash {
    Buffer(GpuBuffer),
    Image(GpuImage),
    Staging(Buffer),
    Set(vk::DescriptorSet),
}

/// Ein noch ausstehender Kopiervorgang.
enum Copy {
    Buffer {
        src: Buffer,
        dst: vk::Buffer,
        size: u64,
    },
    Image {
        src: Buffer,
        dst: vk::Image,
        width: u32,
        height: u32,
        mips: u32,
    },
}

impl Copy {
    fn source(self) -> Buffer {
        match self {
            Copy::Buffer { src, .. } | Copy::Image { src, .. } => src,
        }
    }

    /// Handle des Ziels.
    fn target(&self) -> u64 {
        match self {
            Copy::Buffer { dst, .. } => dst.as_raw(),
            Copy::Image { dst, .. } => dst.as_raw(),
        }
    }
}

/// Alles Nachladbare.
pub struct Store {
    pub arena: Arena,
    models: Vec<Option<Model>>,
    textures: Vec<Option<GpuImage>>,
    materials: Vec<Option<MaterialSlot>>,
    /// wird bei jeder Aenderung an Materialien groesser
    pub version: u64,
    pub sets: MaterialSets,
    pending: std::collections::VecDeque<Copy>,
    /// so viele Bytes je Bild hochladen (mindestens ein Auftrag)
    pub budget: u64,
    /// Auftraege dieses Bildes
    chosen: Vec<Copy>,
    /// Texturen, deren Inhalt noch nicht da ist
    waiting: Vec<TextureId>,
    trash: Vec<(u64, Trash)>,
    frame: u64,
    /// weisses Bild und flache Normale fuer fehlende Texturen
    pub white: TextureId,
    pub flat: TextureId,
}

fn put<T>(list: &mut Vec<Option<T>>, value: T) -> u32 {
    match list.iter().position(Option::is_none) {
        Some(i) => {
            list[i] = Some(value);
            i as u32
        }
        None => {
            list.push(Some(value));
            list.len() as u32 - 1
        }
    }
}

/// Anzahl Stufen bis hinunter zu einem Bildpunkt.
pub fn mip_count(width: u32, height: u32) -> u32 {
    width.max(height).max(1).ilog2() + 1
}

impl Store {
    pub fn new(ctx: &Context) -> Result<Self> {
        let mut s = Self {
            arena: Arena::default(),
            models: Vec::new(),
            textures: Vec::new(),
            materials: Vec::new(),
            version: 1,
            sets: MaterialSets::new(ctx)?,
            pending: Default::default(),
            budget: u64::MAX,
            chosen: Vec::new(),
            waiting: Vec::new(),
            trash: Vec::new(),
            frame: 0,
            white: TextureId(0),
            flat: TextureId(0),
        };
        let pixel = |rgba: [u8; 4]| TextureData {
            width: 1,
            height: 1,
            rgba: rgba.to_vec(),
            srgb: false,
            mips: false,
        };
        s.white = s.add_texture(ctx, &pixel([255; 4]))?;
        s.flat = s.add_texture(ctx, &pixel([128, 128, 255, 255]))?;
        s.waiting.clear(); // die Platzhalter kommen als Erste
        Ok(s)
    }

    fn throw(&mut self, t: Trash) {
        self.trash.push((self.frame, t));
    }

    /// Daten in einen Zwischenpuffer legen.
    fn stage(&mut self, ctx: &Context, bytes: &[u8]) -> Result<Buffer> {
        let usage = vk::BufferUsageFlags::TRANSFER_SRC;
        let b = Buffer::new_host(ctx, bytes.len().max(16) as u64, usage)?;
        b.write(bytes);
        Ok(b)
    }

    fn upload_buffer(
        &mut self,
        ctx: &Context,
        bytes: &[u8],
        usage: vk::BufferUsageFlags,
    ) -> Result<GpuBuffer> {
        let dst = self.arena.buffer(ctx, bytes.len() as u64, usage)?;
        let src = self.stage(ctx, bytes)?;
        self.pending.push_back(Copy::Buffer {
            src,
            dst: dst.buffer,
            size: bytes.len() as u64,
        });
        Ok(dst)
    }

    pub fn add_model(&mut self, ctx: &Context, m: &ModelData) -> Result<ModelId> {
        if m.vertices.is_empty() || m.indices.is_empty() {
            return Err("Modell ohne Dreiecke".into());
        }
        if m.indices.iter().any(|&i| i as usize >= m.vertices.len()) {
            return Err("Modell verweist auf fehlende Eckpunkte".into());
        }
        if !m.joints.is_empty() && m.joints.len() != m.vertices.len() {
            return Err("Modell: Knochen passen nicht zu den Eckpunkten".into());
        }
        let mut vertex = vk::BufferUsageFlags::VERTEX_BUFFER;
        let mut index = vk::BufferUsageFlags::INDEX_BUFFER;
        if ctx.rt.is_some() {
            let rays = vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                | vk::BufferUsageFlags::STORAGE_BUFFER;
            vertex |= rays;
            index |= rays;
        }
        let (center, radius) = m.bounds();
        let model = Model {
            vertices: self.upload_buffer(ctx, bytemuck::cast_slice(&m.vertices), vertex)?,
            indices: self.upload_buffer(ctx, bytemuck::cast_slice(&m.indices), index)?,
            joints: if m.joints.is_empty() {
                None
            } else {
                Some(self.upload_buffer(ctx, bytemuck::cast_slice(&m.joints), vertex)?)
            },
            index_count: m.indices.len() as u32 / 3 * 3,
            bounds: Sphere { center, radius },
            ready: false,
        };
        Ok(ModelId(put(&mut self.models, model)))
    }

    pub fn add_texture(&mut self, ctx: &Context, t: &TextureData) -> Result<TextureId> {
        let (w, h) = (t.width, t.height);
        if w == 0 || h == 0 || t.rgba.len() as u64 != w as u64 * h as u64 * 4 {
            return Err(format!("Textur {w}x{h}: falsche Datenmenge {}", t.rgba.len()).into());
        }
        let mips = if t.mips { mip_count(w, h) } else { 1 };
        let format = if t.srgb {
            vk::Format::R8G8B8A8_SRGB
        } else {
            vk::Format::R8G8B8A8_UNORM
        };
        let usage = vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::TRANSFER_SRC;
        let desc = TexDesc {
            usage,
            mips,
            ..TexDesc::target(w, h, format)
        };
        let image = self.arena.image(ctx, &desc)?;
        let src = self.stage(ctx, &t.rgba)?;
        self.pending.push_back(Copy::Image {
            src,
            dst: image.image,
            width: w,
            height: h,
            mips,
        });
        let id = TextureId(put(&mut self.textures, image));
        self.waiting.push(id);
        Ok(id)
    }

    pub fn add_material(&mut self, ctx: &Context, m: &Material) -> Result<MaterialId> {
        let set = self.sets.alloc(ctx)?;
        let id = put(&mut self.materials, MaterialSlot { data: *m, set });
        self.write_material(ctx, id);
        self.version += 1;
        Ok(MaterialId(id))
    }

    /// Material aendern: neues Set, damit Bilder im Flug das alte behalten.
    pub fn set_material(&mut self, ctx: &Context, id: MaterialId, m: &Material) -> Result<()> {
        if self.material(id).is_none() {
            return Err("unbekanntes Material".into());
        }
        let set = self.sets.alloc(ctx)?;
        let new = MaterialSlot { data: *m, set };
        if let Some(old) = self.materials[id.0 as usize].replace(new) {
            self.throw(Trash::Set(old.set));
        }
        self.write_material(ctx, id.0);
        self.version += 1;
        Ok(())
    }

    fn write_material(&self, ctx: &Context, id: u32) {
        let Some(Some(slot)) = self.materials.get(id as usize) else {
            return;
        };
        let m = &slot.data;
        let view = |t: Option<TextureId>, none: TextureId| {
            t.and_then(|t| self.texture(t))
                .or_else(|| self.texture(none))
                .map_or(vk::ImageView::null(), |i| i.view)
        };
        let views = [
            view(m.base_texture, self.white),
            view(m.metal_rough_texture, self.white),
            view(m.normal_texture, self.flat),
            view(m.occlusion_texture, self.white),
            view(m.emissive_texture, self.white),
        ];
        self.sets.write(ctx, slot.set, views);
    }

    /// Zahlenwerte aller Materialien, mindestens eines.
    pub fn material_values(&self) -> Vec<GpuMaterial> {
        let rays = self.ray_texture_ids();
        self.materials
            .iter()
            .map(|s| {
                let m = s.as_ref().map_or(Material::default(), |s| s.data);
                let normal = if m.normal_texture.is_some() {
                    m.normal_scale
                } else {
                    0.0
                };
                let cut = match m.alpha {
                    Alpha::Mask(c) => c.max(0.001),
                    _ => 0.0,
                };
                GpuMaterial {
                    base: m.base_color.to_array(),
                    emissive: m.emissive.extend(normal).to_array(),
                    params: [m.metallic, m.roughness, cut, ray_texture(&rays, &m)],
                }
            })
            .collect()
    }

    /// Farbtexturen, die Strahlen sehen, in fester Reihenfolge.
    fn ray_texture_ids(&self) -> Vec<TextureId> {
        let mut ids: Vec<TextureId> = self
            .materials
            .iter()
            .flatten()
            .filter_map(|m| m.data.base_texture)
            .filter(|t| self.texture(*t).is_some())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.truncate(super::rt::MAX_TEXTURES);
        ids
    }

    /// Ansichten dazu.
    pub fn ray_textures(&self) -> impl Iterator<Item = vk::ImageView> + '_ {
        let ids = self.ray_texture_ids();
        ids.into_iter()
            .filter_map(|t| self.texture(t).map(|i| i.view))
    }

    pub fn model(&self, id: ModelId) -> Option<&Model> {
        self.models.get(id.0 as usize)?.as_ref()
    }

    /// Wie [`Store::model`], aber nur, wenn es schon gezeichnet werden kann.
    pub fn ready_model(&self, id: ModelId) -> Option<&Model> {
        self.model(id).filter(|m| m.ready)
    }

    pub fn texture(&self, id: TextureId) -> Option<&GpuImage> {
        self.textures.get(id.0 as usize)?.as_ref()
    }

    pub fn material(&self, id: MaterialId) -> Option<&MaterialSlot> {
        self.materials.get(id.0 as usize)?.as_ref()
    }

    /// Ausstehende Auftraege fuer ein Ziel vergessen (es wird freigegeben).
    fn cancel(&mut self, target: u64) {
        let all = std::mem::take(&mut self.pending);
        for c in all {
            if c.target() == target {
                self.throw(Trash::Staging(c.source()));
            } else {
                self.pending.push_back(c);
            }
        }
    }

    pub fn remove_model(&mut self, id: ModelId) {
        if let Some(m) = self.models.get_mut(id.0 as usize).and_then(Option::take) {
            for b in [Some(m.vertices), Some(m.indices), m.joints]
                .into_iter()
                .flatten()
            {
                self.cancel(b.buffer.as_raw());
                self.throw(Trash::Buffer(b));
            }
        }
    }

    pub fn remove_material(&mut self, id: MaterialId) {
        let slot = self.materials.get_mut(id.0 as usize);
        if let Some(m) = slot.and_then(Option::take) {
            self.throw(Trash::Set(m.set));
        }
    }

    /// Textur entfernen; Materialien, die sie benutzen, werden ohne sie neu geschrieben.
    pub fn remove_texture(&mut self, ctx: &Context, id: TextureId) -> Result<()> {
        if id == self.white || id == self.flat {
            return Ok(());
        }
        let slot = self.textures.get_mut(id.0 as usize);
        let Some(image) = slot.and_then(Option::take) else {
            return Ok(());
        };
        self.cancel(image.image.as_raw());
        self.throw(Trash::Image(image));
        let users: Vec<(u32, Material)> = self
            .materials
            .iter()
            .enumerate()
            .filter_map(|(i, s)| Some((i as u32, s.as_ref()?.data)))
            .filter(|(_, m)| textures_of(m).contains(&Some(id)))
            .collect();
        for (i, mut m) in users {
            for t in [
                &mut m.base_texture,
                &mut m.metal_rough_texture,
                &mut m.normal_texture,
                &mut m.occlusion_texture,
                &mut m.emissive_texture,
            ] {
                if *t == Some(id) {
                    *t = None;
                }
            }
            self.set_material(ctx, MaterialId(i), &m)?;
        }
        Ok(())
    }

    /// Anfang eines Bildes: Altes freigeben, dann Ausstehendes kopieren.
    ///
    /// # Safety
    /// `cmd` muss aufgenommen werden; das Bild vor `FRAMES_IN_FLIGHT` Bildern ist fertig.
    pub unsafe fn begin_frame(&mut self, ctx: &Context, cmd: vk::CommandBuffer) {
        self.frame += 1;
        let done = self.frame.saturating_sub(FRAMES_IN_FLIGHT as u64 + 1);
        let (old, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.trash)
            .into_iter()
            .partition(|(f, _)| *f < done);
        self.trash = keep;
        for (_, t) in old {
            self.release(ctx, t);
        }
        let copies = std::mem::take(&mut self.chosen);
        if copies.is_empty() {
            return;
        }
        for c in &copies {
            match c {
                Copy::Buffer { src, dst, size } => {
                    let region = vk::BufferCopy::default().size(*size);
                    ctx.device.cmd_copy_buffer(cmd, src.buffer, *dst, &[region]);
                }
                Copy::Image {
                    src,
                    dst,
                    width,
                    height,
                    mips,
                } => copy_image(&ctx.device, cmd, src.buffer, *dst, (*width, *height, *mips)),
            }
        }
        for c in copies {
            self.throw(Trash::Staging(c.source()));
        }
        let read = vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::VERTEX_ATTRIBUTE_READ | vk::AccessFlags::INDEX_READ);
        ctx.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::VERTEX_INPUT,
            vk::DependencyFlags::empty(),
            &[read],
            &[],
            &[],
        );
    }

    /// Auftraege dieses Bildes waehlen (vor dem Sammeln der Objekte); was damit vollstaendig ist,
    /// gilt als da.
    pub fn choose_copies(&mut self) {
        let mut left = self.budget;
        let mut list = Vec::new();
        while let Some(c) = self.pending.front() {
            let size = match c {
                Copy::Buffer { size, .. } => *size,
                Copy::Image { width, height, .. } => *width as u64 * *height as u64 * 4,
            };
            if !list.is_empty() && size > left {
                break;
            }
            left = left.saturating_sub(size);
            list.extend(self.pending.pop_front());
        }
        if list.is_empty() {
            return;
        }
        let open: std::collections::HashSet<u64> = self.pending.iter().map(Copy::target).collect();
        for m in self.models.iter_mut().flatten().filter(|m| !m.ready) {
            let parts = [Some(m.vertices), Some(m.indices), m.joints];
            let mut parts = parts.into_iter().flatten();
            m.ready = parts.all(|b| !open.contains(&b.buffer.as_raw()));
        }
        let textures = &self.textures;
        self.waiting.retain(|id| {
            let t = textures.get(id.0 as usize).and_then(Option::as_ref);
            t.is_some_and(|i| open.contains(&i.image.as_raw()))
        });
        self.chosen.append(&mut list);
    }

    /// Wartet noch etwas darauf, hochgeladen zu werden?
    pub fn loading(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Ist der Inhalt einer Textur noch unterwegs?
    pub fn is_waiting(&self, m: &Material) -> bool {
        let used = textures_of(m);
        self.waiting.iter().any(|w| used.contains(&Some(*w)))
    }

    unsafe fn release(&mut self, ctx: &Context, t: Trash) {
        let d = &ctx.device;
        match t {
            Trash::Buffer(b) => self.arena.drop_buffer(d, &b),
            Trash::Image(i) => self.arena.drop_image(d, &i),
            Trash::Staging(b) => b.destroy(d),
            Trash::Set(s) => self.sets.free(d, s),
        }
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn destroy(&mut self, ctx: &Context) {
        for i in 0..self.models.len() {
            self.remove_model(ModelId(i as u32));
        }
        let rest = std::mem::take(&mut self.pending);
        let chosen = std::mem::take(&mut self.chosen);
        for c in rest.into_iter().chain(chosen) {
            self.throw(Trash::Staging(c.source()));
        }
        for t in std::mem::take(&mut self.textures).into_iter().flatten() {
            self.throw(Trash::Image(t));
        }
        for (_, t) in std::mem::take(&mut self.trash) {
            self.release(ctx, t);
        }
        self.sets.destroy(&ctx.device);
    }
}

/// Nummer der Farbtextur im Feld der Strahlen plus 1 (0 = keine).
fn ray_texture(ids: &[TextureId], m: &Material) -> f32 {
    m.base_texture
        .and_then(|t| ids.binary_search(&t).ok())
        .map_or(0.0, |i| i as f32 + 1.0)
}

fn textures_of(m: &Material) -> [Option<TextureId>; 5] {
    [
        m.base_texture,
        m.metal_rough_texture,
        m.normal_texture,
        m.occlusion_texture,
        m.emissive_texture,
    ]
}
