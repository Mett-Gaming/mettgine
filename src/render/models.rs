//! Modelle, Texturen und Materialien der modernen Grafik laden und entfernen (jederzeit, ohne
//! auf die GPU zu warten).

use super::*;

impl Renderer {
    /// Legt die moderne Grafik an, falls noetig; `f` bekommt ihren Speicher.
    fn with_store<T>(
        &mut self,
        f: impl FnOnce(&mut modern::Modern, &Context) -> Result<T>,
    ) -> Result<T> {
        let mut m = match self.modern.take() {
            Some(m) => m,
            None => Box::new(modern::Modern::new(&self.ctx, self.command_pool)?),
        };
        let result = f(&mut m, &self.ctx);
        self.modern = Some(m);
        result
    }

    /// Modell laden. Es ist ab dem naechsten Bild zu sehen.
    pub fn add_model(&mut self, m: &ModelData) -> Result<ModelId> {
        self.with_store(|s, ctx| s.store.add_model(ctx, m))
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
            m.store.remove_model(id);
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
