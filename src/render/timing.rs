//! Zeitmessung auf der Grafikkarte (Zeitstempel-Abfragen), gemeinsam fuer `render` und `scene`.
//!
//! Ein Bild wird erst ausgelesen, wenn sein Fence signalisiert ist: gemessen wird ohne Warten,
//! dafuer um die Zahl der Bilder im Flug verzoegert.

use super::context::Context;
use super::Result;
use crate::profile::{FrameStats, PassTime, MAX_PASSES};
use ash::vk;
use std::cell::Cell;

/// Abfragen pro Bild: Start plus Ende jedes Abschnitts
const QUERIES: u32 = MAX_PASSES as u32 + 1;

/// Welche Abschnitte ein Bild im Flug gemessen hat.
#[derive(Clone, Default)]
struct Marks {
    names: Cell<[&'static str; MAX_PASSES]>,
    count: Cell<u32>,
}

/// Zeitstempel fuer alle Bilder im Flug. Ohne Unterstuetzung der Karte bleibt alles 0.
pub(crate) struct GpuTimer {
    pool: Option<vk::QueryPool>,
    /// Nanosekunden pro Zaehlschritt
    period: f32,
    mask: u64,
    marks: Vec<Marks>,
}

impl GpuTimer {
    /// # Safety
    /// `ctx` muss ein gueltiges Geraet enthalten.
    pub unsafe fn new(ctx: &Context, frames: usize) -> Result<Self> {
        let limits = ctx
            .instance
            .get_physical_device_properties(ctx.physical)
            .limits;
        let bits = ctx
            .instance
            .get_physical_device_queue_family_properties(ctx.physical)
            .get(ctx.queue_family as usize)
            .map_or(0, |f| f.timestamp_valid_bits);
        let pool = if bits > 0 && limits.timestamp_period > 0.0 {
            Some(
                ctx.device.create_query_pool(
                    &vk::QueryPoolCreateInfo::default()
                        .query_type(vk::QueryType::TIMESTAMP)
                        .query_count(QUERIES * frames as u32),
                    None,
                )?,
            )
        } else {
            None
        };
        Ok(Self {
            pool,
            period: limits.timestamp_period,
            mask: if bits >= 64 {
                u64::MAX
            } else {
                (1u64 << bits) - 1
            },
            marks: vec![Marks::default(); frames],
        })
    }

    /// Liest das vorige Bild dieses Platzes aus (nach dem Warten auf seinen Fence).
    ///
    /// # Safety
    /// Die GPU muss mit dem Bild `fi` fertig sein.
    pub unsafe fn collect(&self, d: &ash::Device, fi: usize, out: &mut FrameStats) {
        let m = &self.marks[fi];
        let n = m.count.get() as usize;
        let Some(pool) = self.pool.filter(|_| n > 0) else {
            return;
        };
        let mut t = [0u64; QUERIES as usize];
        let first = fi as u32 * QUERIES;
        let flags = vk::QueryResultFlags::TYPE_64;
        if d.get_query_pool_results(pool, first, &mut t[..n + 1], flags)
            .is_err()
        {
            return; // noch nicht bereit: alte Werte behalten
        }
        let ms = |a: u64, b: u64| (b.wrapping_sub(a) & self.mask) as f32 * self.period / 1e6;
        out.passes = [PassTime::default(); MAX_PASSES];
        for (i, name) in m.names.get().iter().take(n).enumerate() {
            out.passes[i] = PassTime {
                name,
                ms: ms(t[i], t[i + 1]),
            };
        }
        out.gpu_ms = ms(t[0], t[n]);
    }

    /// Messung eines Bildes beginnen (gleich nach Beginn des Befehlspuffers).
    ///
    /// # Safety
    /// `cmd` muss aufgenommen werden und ausserhalb eines Rendering-Abschnitts sein.
    pub unsafe fn begin(&self, d: &ash::Device, cmd: vk::CommandBuffer, fi: usize) {
        let Some(pool) = self.pool else {
            return;
        };
        let first = fi as u32 * QUERIES;
        self.marks[fi].count.set(0);
        d.cmd_reset_query_pool(cmd, pool, first, QUERIES);
        d.cmd_write_timestamp(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, pool, first);
    }

    /// Ende eines Abschnitts (hoechstens [`MAX_PASSES`] pro Bild).
    ///
    /// # Safety
    /// wie [`GpuTimer::begin`].
    pub unsafe fn mark(
        &self,
        d: &ash::Device,
        cmd: vk::CommandBuffer,
        fi: usize,
        name: &'static str,
    ) {
        let m = &self.marks[fi];
        let n = m.count.get();
        let Some(pool) = self.pool.filter(|_| n < MAX_PASSES as u32) else {
            return;
        };
        let mut names = m.names.get();
        names[n as usize] = name;
        m.names.set(names);
        m.count.set(n + 1);
        d.cmd_write_timestamp(
            cmd,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            pool,
            fi as u32 * QUERIES + n + 1,
        );
    }

    /// # Safety
    /// Die GPU darf die Abfragen nicht mehr benutzen.
    pub unsafe fn destroy(&self, d: &ash::Device) {
        if let Some(p) = self.pool {
            d.destroy_query_pool(p, None);
        }
    }
}
