//! Modelle, Texturen und Materialien der modernen Grafik laden und entfernen (jederzeit, ohne
//! auf die GPU zu warten).

use super::*;

impl Renderer {
    pub(super) fn new_modern(&self) -> Result<modern::Modern> {
        let formats = (self.screen_format(), self.setup.color);
        modern::Modern::new(&self.ctx, self.command_pool, formats)
    }

    /// Legt die moderne Grafik an, falls noetig; `f` bekommt ihren Speicher.
    fn with_store<T>(
        &mut self,
        f: impl FnOnce(&mut modern::Modern, &Context) -> Result<T>,
    ) -> Result<T> {
        let mut m = match self.modern.take() {
            Some(m) => m,
            None => Box::new(self.new_modern()?),
        };
        let result = f(&mut m, &self.ctx);
        self.modern = Some(m);
        result
    }

    /// Moderne Grafik schon beim Laden anlegen (Shader uebersetzen), damit das erste Bild damit nicht
    /// stockt. Sonst geschieht das beim ersten Gebrauch.
    pub fn prepare_modern(&mut self) -> Result<()> {
        self.with_store(|_, _| Ok(()))
    }

    /// Hoechstens so viele Megabyte je Bild hochladen (0 = alles sofort, Vorgabe). Was noch fehlt,
    /// wird nicht gezeichnet; siehe [`Renderer::loading`].
    pub fn set_upload_budget(&mut self, megabytes: u32) -> Result<()> {
        let bytes = match megabytes {
            0 => u64::MAX,
            n => (n as u64) << 20,
        };
        self.with_store(|m, _| {
            m.store.budget = bytes;
            Ok(())
        })
    }

    /// Warten noch Modelle oder Texturen aufs Hochladen?
    pub fn loading(&self) -> bool {
        self.modern.as_ref().is_some_and(|m| m.store.loading())
    }

    /// Modell laden. Es ist ab dem naechsten Bild zu sehen.
    pub fn add_model(&mut self, m: &ModelData) -> Result<ModelId> {
        self.with_store(|s, ctx| s.add_model(ctx, m))
    }

    /// Textur laden.
    pub fn add_texture(&mut self, t: &TextureData) -> Result<TextureId> {
        self.with_store(|s, ctx| s.store.add_texture(ctx, t))
    }

    /// Material anlegen.
    pub fn add_material(&mut self, m: &Material) -> Result<MaterialId> {
        self.with_store(|s, ctx| s.store.add_material(ctx, m))
    }

    /// Material aendern (gilt ab dem naechsten Bild).
    pub fn set_material(&mut self, id: MaterialId, m: &Material) -> Result<()> {
        match &mut self.modern {
            Some(modern) => modern.store.set_material(&self.ctx, id, m),
            None => Err("unbekanntes Material".into()),
        }
    }

    /// Modell entfernen; der Speicher wird frei, sobald kein Bild im Flug es benutzt.
    pub fn remove_model(&mut self, id: ModelId) {
        if let Some(m) = &mut self.modern {
            m.remove_model(id);
        }
    }

    /// Material entfernen.
    pub fn remove_material(&mut self, id: MaterialId) {
        if let Some(m) = &mut self.modern {
            m.store.remove_material(id);
        }
    }

    /// Textur entfernen; Materialien, die sie benutzen, zeigen danach ihre reine Farbe.
    pub fn remove_texture(&mut self, id: TextureId) -> Result<()> {
        match &mut self.modern {
            Some(m) => m.store.remove_texture(&self.ctx, id),
            None => Ok(()),
        }
    }
}
