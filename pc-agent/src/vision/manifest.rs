//! Per-model manifest loader. Mirrors SenSÉ's `Outils/Modeles/<racine>/modele.json`
//! format, so the vision package and the image/video models share the
//! same metadata conventions.
//!
//! `modele.json` shape (snake_case, per the agent's durable memory rule):
//! ```json
//! {
//!   "id": "yolox-nano",
//!   "nom": "YOLOX-Nano",
//!   "moteur": "onnxruntime",
//!   "espace": "Vision",
//!   "racine": "vision",
//!   "fichiers": { "poids": "model.onnx" },
//!   "taille_mo": 4,
//!   "vram_mo": 250,
//!   "params_backend": "cpu",
//!   "produit": "detections",
//!   "licence": "Apache-2.0",
//!   "url_poids": "https://huggingface.co/.../yolox_nano.onnx"
//! }
//! ```

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeleManifest {
    pub id: String,
    pub nom: String,
    pub moteur: String,
    pub espace: String,
    pub racine: String,
    pub fichiers: Fichiers,
    #[serde(default)]
    pub taille_mo: u32,
    #[serde(default)]
    pub vram_mo: u32,
    #[serde(default)]
    pub params_backend: Option<String>,
    #[serde(default)]
    pub produit: String,
    #[serde(default)]
    pub licence: Option<String>,
    /// Where to download the weights from. Used by `download` mode.
    #[serde(default)]
    pub url_poids: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Fichiers {
    /// Path of the ONNX model relative to the manifest dir.
    #[serde(default)]
    pub poids: Option<String>,
}

impl ModeleManifest {
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("modele.json");
        let txt = std::fs::read_to_string(&path)
            .with_context(|| format!("lecture {}", path.display()))?;
        let m: Self = serde_json::from_str(&txt)
            .with_context(|| format!("parse {}", path.display()))?;
        Ok(m)
    }

    /// Absolute path of the model weights, or None if the manifest
    /// doesn't declare any (e.g. pure-algorithm models like ByteTrack).
    pub fn poids_absolu(&self, dir: &Path) -> Option<std::path::PathBuf> {
        self.fichiers.poids.as_ref().map(|p| dir.join(p))
    }
}
