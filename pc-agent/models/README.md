# Vision models

Three SenSÉ-compatible vision models live here, each in its own folder with a `modele.json` manifest that mirrors the format used by SenSÉ's `Outils/Modeles/`.

| ID | Modèle | Tâche | Taille | Licence |
|---|---|---|---|---|
| `yolox-nano` | YOLOX-Nano | 80 classes COCO (animaux, personnes, véhicules) | ~4 Mo | Apache 2.0 |
| `yunet` | YuNet (OpenCV Zoo) | Visage + 5 landmarks | ~1 Mo | MIT |

ByteTrack is an algorithm (not a model) — no weights needed.

## Layout

```
models/vision/
├── yolox-nano/
│   ├── modele.json     # manifest (snake_case JSON, see below)
│   └── model.onnx      # weights (downloaded)
├── yunet/
│   ├── modele.json
│   └── model.onnx
└── download.ps1        # one-shot fetcher (idempotent)
```

The pc-agent looks for weights in `<exe_dir>/models/vision/<id>/model.onnx`. Override with the `VISION_MODELS_DIR` env var.

## Download

```powershell
# from the pc-agent repo root
pwsh models/download.ps1
# or one model only
pwsh models/download.ps1 -Only yolox-nano
```

Re-download with `-Force`.

## Manifest format (`modele.json`)

Snake_case, consistent with SenSÉ's `Outils/Modeles/<racine>/modele.json` (see agent memory). The pc-agent's `vision::manifest::ModeleManifest` deserializes this directly.

```json
{
  "id": "yolox-nano",
  "nom": "YOLOX-Nano (COCO 80)",
  "moteur": "onnxruntime",
  "espace": "Vision",
  "racine": "vision",
  "fichiers": { "poids": "model.onnx" },
  "taille_mo": 4,
  "vram_mo": 250,
  "params_backend": "cpu",
  "produit": "detections",
  "licence": "Apache-2.0",
  "url_poids": "https://..."
}
```

`_notes` (snake_case prefixed with `_`) is a documentation field, ignored by the loader.

## Why these models

- **YOLOX-Nano** : smallest MIT/Apache YOLO in production quality. ~30 ms/frame CPU at 640×640.
- **YuNet** : the smallest face detector that ships with landmarks. MIT, 137 KB ONNX, ~2 ms/frame.
- **ByteTrack** (in code) : MIT, ECCV 2022. State-of-the-art multi-object tracker, no weights needed.

YOLOv5/v8/v11 (Ultralytics) are AGPL-3.0 — incompatible with SenSÉ MIT. YOLOv3 is the only MIT YOLO but is obsolete.
